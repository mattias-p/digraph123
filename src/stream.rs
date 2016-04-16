use std;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io;
use std::num::ParseIntError;
use std::path::Path;
use std::str::FromStr;
use vorbis;

pub trait Stream {
    fn is_eos(&self) -> bool;
    fn max_read(&self) -> usize;
    fn read_add(&mut self, buf: &mut [f32]);
    fn load(&mut self) -> Result<Vec<Box<Stream>>, MyError>;
}

pub struct EmptyStream;

impl Stream for EmptyStream {
    fn is_eos(&self) -> bool {
        true
    }

    fn max_read(&self) -> usize {
        0
    }

    fn read_add(&mut self, buf: &mut [f32]) {
        if buf.len() > self.max_read() {
            panic!("out of bounds in EmptyStream");
        }
    }

    fn load(&mut self) -> Result<Vec<Box<Stream>>, MyError> {
        Ok(vec![])
    }
}

pub struct VorbisStream {
    offset: usize,
    packet: Vec<f32>,
    next_packet: Option<Vec<f32>>,
    packets: vorbis::PacketsIntoIter<File>,
}

impl VorbisStream {
    pub fn new(decoder: vorbis::Decoder<File>) -> Result<VorbisStream, MyError> {
        let mut packets = decoder.into_packets();
        let first = if let Some(first) = packets.next() {
            Some(try!(first)
                     .data
                     .iter()
                     .map(|value| *value as f32 / i16::max_value() as f32)
                     .collect())
        } else {
            None
        };
        let mut stream = VorbisStream {
            offset: 0,
            packet: vec![],
            next_packet: first,
            packets: packets,
        };
        try!(stream.load());
        Ok(stream)
    }
}

impl Stream for VorbisStream {
    fn is_eos(&self) -> bool {
        self.next_packet.is_none() && self.max_read() == 0
    }

    fn max_read(&self) -> usize {
        self.packet.len() - self.offset
    }

    fn read_add(&mut self, buf: &mut [f32]) {
        if buf.len() > self.max_read() {
            panic!("out of bounds in VorbisStream");
        }

        let old_offset = self.offset;
        self.offset += buf.len();

        let data = &self.packet[old_offset..self.offset];

        for (out, value) in buf.iter_mut().zip(data) {
            *out += *value;
        }
    }

    fn load(&mut self) -> Result<Vec<Box<Stream>>, MyError> {
        if self.offset == self.packet.len() {
            if let Some(next_packet) = std::mem::replace(&mut self.next_packet, None) {
                let mut recycled = std::mem::replace(&mut self.packet, next_packet);
                let recycled_len = recycled.len();
                self.offset = 0;
                if let Some(vorbis_packet) = self.packets.next() {
                    let data = try!(vorbis_packet).data;
                    if recycled.len() < data.len() {
                        recycled.reserve_exact(data.len() - recycled_len);
                    }
                    recycled.truncate(0);
                    recycled.extend(data.iter()
                                        .map(|value| *value as f32 / i16::max_value() as f32));
                    self.next_packet = Some(recycled);
                }
            }
        }
        Ok(vec![])
    }
}

pub struct Track {
    stream: Box<Stream>,
    splice_point: Option<u64>,
}

impl Track {
    pub fn empty() -> Track {
        Track {
            stream: Box::new(EmptyStream),
            splice_point: None,
        }
    }

    pub fn vorbis(path: &Path) -> Result<Track, MyError> {
        let display = path.display();
        let file = match File::open(&path) {
            Err(why) => panic!("Couldn't open {}: {}", display, Error::description(&why)),
            Ok(file) => file,
        };

        let decoder = try!(vorbis::Decoder::new(file));
        let splice_point = try!(decoder.get_comment("SPLICEPOINT"));
        let splice_point = splice_point.iter()
                                       .fold(Ok(None), |acc, value| {
                                           let res: Result<_, MyError> = acc.and_then(|acc| {
                                               let value = try!(u64::from_str(value));
                                               Ok(acc.map(|acc| std::cmp::min(acc, value))
                                                     .or(Some(value)))
                                           });
                                           res
                                       });
        let splice_point = try!(splice_point);
        let stream = try!(VorbisStream::new(decoder));
        Ok(Track {
            stream: Box::new(stream),
            splice_point: splice_point,
        })
    }

    pub fn splice_point_as_usize(&self) -> Option<usize> {
        self.splice_point.and_then(|sp| {
            if sp <= usize::max_value() as u64 {
                Some(sp as usize)
            } else {
                None
            }
        })
    }
}

impl Stream for Track {
    fn is_eos(&self) -> bool {
        self.stream.is_eos()
    }

    fn max_read(&self) -> usize {
        if let Some(sp) = self.splice_point_as_usize() {
            std::cmp::min(sp, self.stream.max_read())
        } else {
            self.stream.max_read()
        }
    }

    fn read_add(&mut self, buf: &mut [f32]) {
        if buf.len() > self.max_read() {
            panic!("out of bounds in Track");
        }
        self.stream.read_add(buf);
    }

    fn load(&mut self) -> Result<Vec<Box<Stream>>, MyError> {
        if self.max_read() == 0 {
            try!(self.stream.load());
            if self.splice_point == Some(0) {
                let tail = std::mem::replace(&mut self.stream, Box::new(EmptyStream));
                Ok(vec![tail])
            } else {
                Ok(vec![])
            }
        } else {
            Ok(vec![])
        }
    }
}

pub struct Player {
    track: Track,
    lookahead: Option<Track>,
    play_list: Box<Iterator<Item = Result<Track, MyError>>>,
}

impl Player {
    pub fn new(tracks: Box<Iterator<Item = Result<Track, MyError>>>) -> Result<Player, MyError> {
        let mut player = Player {
            track: Track::empty(),
            lookahead: Some(Track::empty()),
            play_list: tracks,
        };
        if player.max_read() == 0 {
            let tails = try!(player.load());
            assert_eq!(tails.len(), 0);
        }
        Ok(player)
    }
}

impl Stream for Player {
    fn is_eos(&self) -> bool {
        self.lookahead.is_none() && self.track.is_eos()
    }

    fn max_read(&self) -> usize {
        self.track.max_read()
    }

    fn read_add(&mut self, buf: &mut [f32]) {
        if buf.len() > self.max_read() {
            panic!("out of bounds in Player");
        }
        self.track.read_add(buf);
    }

    fn load(&mut self) -> Result<Vec<Box<Stream>>, MyError> {
        let mut tails = vec![];
        while self.track.max_read() == 0 {
            let new_tails = self.track.load().map_err(|err| {
                self.track = Track::empty();
                self.lookahead = None;
                err
            });
            tails.extend(try!(new_tails));
            if self.track.is_eos() {
                if let Some(new_track) = self.play_list.next() {
                    self.track = try!(new_track);
                } else {
                    break;
                }
            }
        }
        Ok(tails)
    }
}

pub struct Mixer {
    streams: Vec<Box<Stream>>,
    errors: VecDeque<MyError>,
}

impl Mixer {
    pub fn new(streams: Vec<Box<Stream>>) -> Mixer {
        Mixer {
            errors: VecDeque::with_capacity(streams.len()),
            streams: streams,
        }
    }
}

impl Stream for Mixer {
    fn is_eos(&self) -> bool {
        self.streams.len() == 0 ||
        self.streams
            .iter()
            .all(|stream| stream.is_eos())
    }

    fn max_read(&self) -> usize {
        if self.streams.len() == 0 {
            0
        } else {
            self.streams
                .iter()
                .map(|stream| stream.max_read())
                .fold(usize::max_value(), std::cmp::min)
        }
    }

    fn read_add(&mut self, buf: &mut [f32]) {
        if buf.len() > self.max_read() {
            panic!("out of bounds in Mixer");
        }

        for stream in self.streams.iter_mut() {
            stream.read_add(buf);
        }
    }

    fn load(&mut self) -> Result<Vec<Box<Stream>>, MyError> {
        if let Some(err) = self.errors.pop_front() {
            return Err(err);
        }

        let mut new_tails = vec![];
        let mut empties = vec![];

        for (i, stream) in self.streams.iter_mut().enumerate() {
            if stream.max_read() == 0 {
                match stream.load() {
                    Ok(tails) => {
                        new_tails.extend(tails);
                        if stream.is_eos() {
                            empties.push(i);
                        }
                    }
                    Err(err) => {
                        self.errors.push_back(err);
                        empties.push(i);
                    }
                }
            }
        }

        empties.reverse();

        for i in empties {
            self.streams.swap_remove(i);
        }

        self.streams.extend(new_tails);

        if let Some(err) = self.errors.pop_front() {
            Err(err)
        } else {
            Ok(vec![])
        }
    }
}

#[derive(Debug)]
pub enum MyError {
    Io(io::Error),
    ParseInt(ParseIntError),
    Vorbis(vorbis::VorbisError),
}

impl Error for MyError {
    fn description(&self) -> &str {
        match self {
            &MyError::ParseInt(_) => "A string could not be parsed as an integer",
            &MyError::Vorbis(ref err) => err.description(),
            &MyError::Io(_) => "An I/O error ocurred",
        }
    }

    fn cause(&self) -> Option<&Error> {
        match self {
            &MyError::ParseInt(ref err) => Some(err as &std::error::Error),
            &MyError::Vorbis(ref err) => err.cause(),
            &MyError::Io(ref err) => Some(err as &std::error::Error),
        }
    }
}

impl From<ParseIntError> for MyError {
    fn from(err: ParseIntError) -> MyError {
        MyError::ParseInt(err)
    }
}

impl From<vorbis::VorbisError> for MyError {
    fn from(err: vorbis::VorbisError) -> MyError {
        MyError::Vorbis(err)
    }
}

impl From<io::Error> for MyError {
    fn from(err: io::Error) -> MyError {
        MyError::Io(err)
    }
}

impl fmt::Display for MyError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "{}", Error::description(self))
    }
}
