use std::cmp;
use std::error;
use std::fmt;
use std::fs;
use std::io;
use std::mem;
use std::num;
use std::path;
use std::result;
use std::str::FromStr;
use vorbis;

pub type Result<T> = result::Result<T, Error>;

pub trait Stream {
    fn is_eos(&self) -> bool;
    fn max_read(&self) -> usize;
    fn read_add(&mut self, buf: &mut [f32]);
    fn load(&mut self) -> Result<Vec<Box<Stream>>>;
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

    fn load(&mut self) -> Result<Vec<Box<Stream>>> {
        Ok(vec![])
    }
}

pub struct VorbisStream {
    offset: usize,
    packet: Vec<f32>,
    next_packet: Option<Vec<f32>>,
    packets: vorbis::PacketsIntoIter<fs::File>,
}

impl VorbisStream {
    pub fn new(decoder: vorbis::Decoder<fs::File>) -> Result<VorbisStream> {
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

    fn load(&mut self) -> Result<Vec<Box<Stream>>> {
        if self.offset == self.packet.len() {
            if let Some(next_packet) = mem::replace(&mut self.next_packet, None) {
                let mut recycled = mem::replace(&mut self.packet, next_packet);
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

    pub fn vorbis(path: &path::Path) -> Result<Track> {
        let display = path.display();
        let file = match fs::File::open(&path) {
            Err(why) => {
                panic!("Couldn't open {}: {}",
                       display,
                       error::Error::description(&why))
            }
            Ok(file) => file,
        };

        let decoder = try!(vorbis::Decoder::new(file));
        let splice_point = try!(decoder.get_comment("SPLICEPOINT"));
        let splice_point = splice_point.iter()
                                       .fold(Ok(None), |acc, value| {
                                           let res: Result<_> = acc.and_then(|acc| {
                                               let value = try!(u64::from_str(value));
                                               Ok(acc.map(|acc| cmp::min(acc, value))
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
            cmp::min(sp, self.stream.max_read())
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

    fn load(&mut self) -> Result<Vec<Box<Stream>>> {
        if self.max_read() == 0 {
            try!(self.stream.load());
            if self.splice_point == Some(0) {
                let tail = mem::replace(&mut self.stream, Box::new(EmptyStream));
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
    play_list: Box<Iterator<Item = Result<Track>>>,
}

impl Player {
    pub fn new(tracks: Box<Iterator<Item = Result<Track>>>) -> Result<Player> {
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

    fn load(&mut self) -> Result<Vec<Box<Stream>>> {
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
}

impl Mixer {
    pub fn new(streams: Vec<Box<Stream>>) -> Mixer {
        Mixer { streams: streams }
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
                .fold(usize::max_value(), cmp::min)
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

    fn load(&mut self) -> Result<Vec<Box<Stream>>> {
        let mut tails = vec![];
        let mut empties = vec![];
        let mut errors = vec![];

        for (i, stream) in self.streams.iter_mut().enumerate() {
            match stream.load() {
                Ok(new_tails) => {
                    tails.extend(new_tails);
                    if stream.is_eos() {
                        empties.push(i);
                    }
                }
                Err(err) => {
                    errors.push(err);
                    empties.push(i);
                }
            }
        }

        empties.reverse();

        for i in empties {
            self.streams.swap_remove(i);
        }

        self.streams.extend(tails);

        if errors.is_empty() {
            Ok(vec![])
        } else {
            Err(From::from(errors))
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Parse(num::ParseIntError),
    Vorbis(vorbis::VorbisError),
    Multiple(Vec<Error>),
    AudioFormat,
    File(path::PathBuf, Box<Error>),
    Dir(String, Box<Error>),
    NoItems,
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match self {
            &Error::Io(_) => "an I/O error",
            &Error::Parse(_) => "a parse error",
            &Error::Vorbis(_) => "a Vorbis decoder error",
            &Error::Multiple(_) => "multiple errors",
            &Error::AudioFormat => "inconsistent audio formats",
            &Error::File(_, _) => "an error occurred in a file",
            &Error::Dir(_, _) => "an error occurred in a directory",
            &Error::NoItems => "no items",
        }
    }

    fn cause(&self) -> Option<&error::Error> {
        use std::ops::Deref;
        match self {
            &Error::Io(ref err) => Some(err as &error::Error),
            &Error::Parse(ref err) => Some(err as &error::Error),
            &Error::Vorbis(ref err) => Some(err as &error::Error),
            &Error::File(_, ref err) => Some(err.deref() as &error::Error),
            &Error::Dir(_, ref err) => Some(err.deref() as &error::Error),
            _ => None,
        }
    }
}

impl From<num::ParseIntError> for Error {
    fn from(err: num::ParseIntError) -> Error {
        Error::Parse(err)
    }
}

impl From<vorbis::VorbisError> for Error {
    fn from(err: vorbis::VorbisError) -> Error {
        Error::Vorbis(err)
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::Io(err)
    }
}

impl From<Vec<Error>> for Error {
    fn from(mut errors: Vec<Error>) -> Error {
        if errors.len() > 1 {
            Error::Multiple(errors)
        } else {
            errors.pop().expect("empty list")
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> result::Result<(), fmt::Error> {
        use std::error::Error;
        match self {
            &::stream::Error::Io(_) => write!(f, "{}", self.description()),
            &::stream::Error::Parse(_) => write!(f, "{}", self.description()),
            &::stream::Error::Vorbis(_) => write!(f, "{}", self.description()),
            &::stream::Error::Multiple(ref err) => {
                let parts: Vec<_> = err.iter().map(::stream::Error::to_string).collect();
                write!(f, "{}:\n * {}", self.description(), parts.join("\n * "))
            }
            &::stream::Error::AudioFormat => write!(f, "{}", self.description()),
            &::stream::Error::File(ref path, _) => {
                write!(f, "problem with file '{}'", path.display())
            }
            &::stream::Error::Dir(ref path, _) => write!(f, "problem with directory '{}'", path),
            &::stream::Error::NoItems => write!(f, "{}", self.description()),
        }
    }
}
