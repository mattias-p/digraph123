extern crate clap;
extern crate cpal;
extern crate rand;
extern crate regex;
extern crate vorbis;

#[macro_use]
extern crate lazy_static;

use clap::{Arg, App};
use rand::Rng;
use regex::Regex;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io;
use std::io::{Write, stderr};
use std::num::ParseIntError;
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::process;
use std::str::FromStr;

#[derive(Debug)]
enum MyError {
    Io(io::Error),
    ParseInt(ParseIntError),
    Vorbis(vorbis::VorbisError),
}

impl Error for MyError {
    fn description(&self) -> &str {
        match self {
            &MyError::ParseInt(_) => "A string could not be parsed as an integer",
            &MyError::Vorbis(_) => "An error occured in the Vorbis decoder",
            &MyError::Io(_) => "An I/O error ocurred",
        }
    }
    fn cause(&self) -> Option<&Error> {
        match self {
            &MyError::ParseInt(ref err) => Some(err as &std::error::Error),
            &MyError::Vorbis(ref err) => Some(err as &std::error::Error),
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

trait Stream {
    fn min_bound(&self) -> usize;
    fn max_bound(&self) -> Option<usize>;
    fn next_slice(&mut self, usize) -> Result<&[f32], MyError>;
    fn get_tails(&mut self) -> Result<Vec<Box<Stream>>, MyError>;
}

pub struct VorbisStream {
    offset: usize,
    packet: Vec<f32>,
    next_packet: Option<Vec<f32>>,
    packets: vorbis::PacketsIntoIter<File>,
}

impl Stream for VorbisStream {
    fn next_slice(&mut self, size: usize) -> Result<&[f32], MyError> {
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
        let min = self.min_bound();
        if size > min {
            panic!("out of bounds in VorbisStream");
        }
        let old_offset = self.offset;
        self.offset += size;
        Ok(&self.packet[old_offset..self.offset])
    }

    fn min_bound(&self) -> usize {
        if self.offset == self.packet.len() {
            if let Some(ref packet) = self.next_packet {
                packet.len()
            } else {
                0
            }
        } else {
            self.packet.len() - self.offset
        }
    }

    fn max_bound(&self) -> Option<usize> {
        if self.next_packet.is_none() {
            Some(self.min_bound())
        } else {
            None
        }
    }

    fn get_tails(&mut self) -> Result<Vec<Box<Stream>>, MyError> {
        Ok(vec![])
    }
}

static NO_FLOATS: [f32; 0] = [];

struct EmptyStream([f32; 0]);

impl EmptyStream {
    fn new() -> EmptyStream {
        EmptyStream(NO_FLOATS)
    }
}

impl Stream for EmptyStream {
    fn next_slice(&mut self, size: usize) -> Result<&[f32], MyError> {
        if size == 0 {
            Ok(&self.0)
        } else {
            panic!("out of bounds in EmptyStream");
        }
    }
    fn min_bound(&self) -> usize {
        0
    }
    fn max_bound(&self) -> Option<usize> {
        Some(0)
    }
    fn get_tails(&mut self) -> Result<Vec<Box<Stream>>, MyError> {
        Ok(vec![])
    }
}

struct Track {
    stream: Box<Stream>,
    splice_point: Option<u64>,
}

impl Track {
    fn splice_point_as_usize(&self) -> Option<usize> {
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
    fn next_slice(&mut self, size: usize) -> Result<&[f32], MyError> {
        let min = self.min_bound();
        if size > min {
            panic!("out of bounds in Track");
        }
        self.splice_point = self.splice_point.map(|sp| sp - size as u64);
        self.stream.next_slice(size)
    }

    fn min_bound(&self) -> usize {
        let min = self.stream.min_bound();
        let sp = self.splice_point_as_usize();
        if let Some(sp) = sp {
            std::cmp::min(sp, min)
        } else {
            min
        }
    }

    fn max_bound(&self) -> Option<usize> {
        let max = self.stream.max_bound();
        let sp = self.splice_point_as_usize();
        if let (Some(sp), Some(max)) = (sp, max) {
            Some(std::cmp::min(sp, max))
        } else {
            sp.or(max)
        }
    }

    fn get_tails(&mut self) -> Result<Vec<Box<Stream>>, MyError> {
        if self.max_bound() == Some(0) {
            let tail = std::mem::replace(&mut self.stream, Box::new(EmptyStream::new()));
            if tail.max_bound() != Some(0) {
                Ok(vec![tail])
            } else {
                Ok(vec![])
            }
        } else {
            panic!("unconsumed data");
        }
    }
}

struct Digraph(Vec<Vec<(usize, Vec<PathBuf>)>>);

impl Digraph {
    fn into_random_walk(self, rng: Box<Rng>) -> IntoRandomWalk {
        IntoRandomWalk {
            state: 0,
            digraph: self,
            rng: rng,
        }
    }
    fn print(&self) {
        for (tail, heads) in self.0.iter().enumerate() {
            for item in heads {
                let &(ref head, ref paths) = item;
                for path in paths {
                    println!("{} -> {}: {}", tail + 1, head + 1, path.display());
                }
            }
        }
    }
}

struct IntoRandomWalk {
    state: usize,
    digraph: Digraph,
    rng: Box<Rng>,
}

impl IntoRandomWalk {
    fn next_once(&mut self) -> Option<&Path> {
        let ref mut rng = self.rng;
        let cells = self.digraph.0.get(self.state);
        if let Some(&(new_state, ref arrows)) = cells.and_then(|cells| rng.choose(cells)) {
            self.state = new_state;
            rng.choose(arrows.as_slice()).map(|path| path.as_path())
        } else {
            None
        }
    }
}

impl<'a> Iterator for IntoRandomWalk {
    type Item = PathBuf;
    fn next(&mut self) -> Option<PathBuf> {
        let path = self.next_once().map(|p| p.to_path_buf());
        path.or_else(|| self.next_once().map(|p| p.to_path_buf()))
    }
}

fn vorbis_track(path: &Path) -> Result<Track, MyError> {
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
    let stream = VorbisStream {
        offset: 0,
        packet: vec![],
        next_packet: first,
        packets: packets,
    };
    Ok(Track {
        stream: Box::new(stream),
        splice_point: splice_point,
    })
}

struct Player {
    track: Track,
    play_list: Box<Iterator<Item = Result<Track, MyError>>>,
}

impl Player {
    fn new(tracks: Box<Iterator<Item = Result<Track, MyError>>>) -> Result<Player, MyError> {
        let mut player = Player {
            track: Track {
                stream: Box::new(EmptyStream::new()),
                splice_point: None,
            },
            play_list: tracks,
        };
        if let Some(0) = player.max_bound() {
            try!(player.get_tails());
        }
        Ok(player)
    }
}

impl Stream for Player {
    fn min_bound(&self) -> usize {
        self.track.min_bound()
    }
    fn max_bound(&self) -> Option<usize> {
        self.track.max_bound()
    }
    fn next_slice(&mut self, size: usize) -> Result<&[f32], MyError> {
        self.track.next_slice(size)
    }
    fn get_tails(&mut self) -> Result<Vec<Box<Stream>>, MyError> {
        let mut tails = vec![];
        while self.track.max_bound() == Some(0) {
            tails.extend(try!(self.track.get_tails()));
            if let Some(new_track) = self.play_list.next() {
                self.track = try!(new_track);
            } else {
                break;
            }
        }
        Ok(tails)
    }
}

struct Mixer {
    coefficient: f32,
    streams: Vec<Box<Stream>>,
}

impl Mixer {
    fn new(streams: Vec<Box<Stream>>) -> Mixer {
        Mixer {
            coefficient: 1.0 / streams.len() as f32,
            streams: streams,
        }
    }
    fn mix_next_slice(&mut self, buf: &mut [f32]) -> Vec<MyError> {
        let size = buf.len();
        let coefficient = self.coefficient;
        for out in buf.iter_mut() {
            *out = 0.0;
        }
        self.each_next_slice(size,
                             &mut |slice| {
                                 for (out, value) in buf.iter_mut().zip(slice) {
                                     *out += coefficient * *value;
                                 }
                             })
    }
    fn each_next_slice(&mut self, size: usize, f: &mut FnMut(&[f32])) -> Vec<MyError> {
        let mut errors = vec![];
        for stream in self.streams.iter_mut() {
            match stream.next_slice(size) {
                Err(err) => errors.push(err),
                Ok(slice) => f(slice),
            }
        }

        let mut new_tails = vec![];
        let mut empties = vec![];
        for (i, stream) in self.streams.iter_mut().enumerate() {
            if stream.min_bound() == 0 {
                match stream.get_tails() {
                    Ok(tails) => {
                        new_tails.extend(tails);
                        if stream.max_bound() == Some(0) {
                            empties.push(i);
                        }
                    }
                    Err(err) => {
                        errors.push(err);
                    }
                }
            }
        }
        empties.reverse();
        for i in empties {
            self.streams.swap_remove(i);
        }
        self.streams.extend(new_tails);

        if errors.len() == 0 {
            vec![]
        } else {
            errors
        }
    }
    fn min_bound(&self) -> usize {
        self.streams
            .iter()
            .fold(None as Option<usize>, |acc, stream| {
                let min = stream.min_bound();
                acc.map(|acc| std::cmp::min(acc, min))
                   .or(Some(min))
            })
            .unwrap_or(0)
    }
    fn max_bound(&self) -> Option<usize> {
        self.streams
            .iter()
            .fold(None as Option<Option<usize>>, |acc, stream| {
                let max = stream.max_bound();
                if let Some(acc_max) = acc {
                    if let (Some(acc_max), Some(max)) = (acc_max, max) {
                        Some(Some(std::cmp::min(acc_max, max)))
                    } else {
                        Some(acc_max.or(max))
                    }
                } else {
                    Some(max)
                }
            })
            .unwrap_or(Some(0))
    }
}

macro_rules! insist {
    ($res:expr, $fmt:tt $(, $arg:expr)*) => {
        match $res {
            Ok(value) => value,
            Err(ref err) => {
                let prog_name = &std::env::args().next().expect("std::env::args()");
                let prog_name = Path::new(prog_name).file_name().expect("file_name").to_string_lossy();
                writeln!(&mut stderr(), concat!("{}: error: ", $fmt, ": {}"), prog_name $(, $arg)*, err.description()).
ok();
                process::exit(1);
            }
        }
    }   
}

fn path_to_section(path: &Path) -> Option<(String, String, Option<String>)> {
    lazy_static! {
        static ref SECTION_RE: Regex = Regex::new(r"^([^-]+)-([^-]+)(?:-(.+))?.ogg$").unwrap();
    }
    path.file_name()
        .and_then(|os_str| os_str.to_str())
        .and_then(|file_name| SECTION_RE.captures(file_name))
        .map(|cap| {
            (cap[1].to_lowercase().to_string(),
             cap[2].to_lowercase().to_string(),
             cap.at(3).map(|s| s.to_string()))
        })
}

fn path_to_stream_config(path: &Path) -> Result<(u8, u32), MyError> {
    let file = try!(File::open(path));
    let mut decoder = try!(vorbis::Decoder::new(file));
    let packet = try!(decoder.packets().next().expect("first packet"));
    Ok((packet.channels as u8, packet.rate as u32))
}

struct DigraphBuilder {
    indices: HashMap<String, usize>,
    arrows: HashMap<(usize, usize), Vec<PathBuf>>,
}

impl DigraphBuilder {
    fn new() -> DigraphBuilder {
        let mut indices = HashMap::new();
        indices.insert("start".to_string(), 0);
        DigraphBuilder {
            indices: indices,
            arrows: HashMap::new(),
        }
    }
    fn arrow(mut self, tail: String, head: String, path: PathBuf) -> Self {
        let next_index = self.indices.len();
        let tail = *self.indices.entry(tail).or_insert(next_index);
        let next_index = self.indices.len();
        let head = *self.indices.entry(head).or_insert(next_index);
        self.arrows
            .entry((tail, head))
            .or_insert_with(|| vec![])
            .push(path);
        self
    }
}

impl Into<Digraph> for DigraphBuilder {
    fn into(self) -> Digraph {
        let mut digraph = Vec::with_capacity(self.indices.len());
        for _ in 0..self.indices.len() {
            digraph.push(vec![]);
        }
        for ((tail, head), arrows) in self.arrows {
            digraph[tail].push((head, arrows));
        }
        if digraph[0].len() == 0 {
            for i in 1..self.indices.len() {
                digraph[0].push((i, vec![]));
            }
        }
        Digraph(digraph)
    }
}

fn main() {
    let mut channel_stream_config = None;
    let matches = App::new("digraph123")
                      .version("1.0.0")
                      .author("Mattias Päivärinta")
                      .about("Play digraph shaped audio recordings using random walk")
                      .arg(Arg::with_name("dir")
                               .help("A digraph directory")
                               .index(1)
                               .multiple(true))
                      .get_matches();
    let dirs: Vec<_> = matches.values_of("dir").map(|v| v.collect()).unwrap_or(vec![]);
    let mut streams: Vec<Box<Stream>> = vec![];
    for dir in dirs {
        let dir_files = insist!(std::fs::read_dir(dir), "reading directory '{}'", dir);
        let mut digraph_builder = DigraphBuilder::new();
        for entry in dir_files {
            let entry = insist!(entry, "traversing directory '{}'", dir);
            let path = entry.path();
            let path_display = path.display();
            if let Some((tail, head, _)) = path_to_section(&path) {
                let file_stream_config = insist!(path_to_stream_config(&path),
                                                 "getting stream config of '{}'",
                                                 path_display);
                let file_stream_config = Some(file_stream_config);
                channel_stream_config = channel_stream_config.or(file_stream_config);
                if file_stream_config == channel_stream_config {
                    digraph_builder = digraph_builder.arrow(tail, head, path.clone());
                }
            }
        }
        let digraph: Digraph = digraph_builder.into();
        let tracks = digraph.into_random_walk(Box::new(rand::thread_rng()))
                            .map(|p| vorbis_track(p.as_path()));
        let stream = Player::new(Box::new(tracks)).unwrap();

        streams.push(Box::new(stream));
    }
    let mut mixer = Mixer::new(streams);

    let channel_stream_config = channel_stream_config.unwrap();

    let endpoint = cpal::get_default_endpoint().expect("default endpoint");
    let format = {
        let formats = endpoint.get_supported_formats_list();
        let formats = insist!(formats,
                              "getting list of formats supported by default endpoint");

        formats.filter(|f| f.samples_rate.0 as u32 == channel_stream_config.1)
               .filter(|f| f.channels.len() == channel_stream_config.0 as usize)
               .filter(|f| f.data_type == cpal::SampleFormat::F32)
               .next()
    };
    let format = if let Some(format) = format {
        format
    } else {
        panic!("stream format not supported");
    };

    let mut channel = cpal::Voice::new(&endpoint, &format).expect("Failed to create a channel");

    let num_channels = channel_stream_config.0 as usize;

    loop {
        // since we just called peek(), min_size will be non-zero, and append_data() will be happy
        let min_size = mixer.min_bound();
        let min_size = min_size + (num_channels - 1 - (min_size + 1) % num_channels);
        match channel.append_data(min_size) {
            cpal::UnknownTypeBuffer::F32(mut buffer) => {
                mixer.mix_next_slice(buffer.deref_mut());
            }   

            cpal::UnknownTypeBuffer::U16(_) => {
                panic!("unsupported buffer type");
            }   

            cpal::UnknownTypeBuffer::I16(_) => {
                panic!("unsupported buffer type");
            }   
        };

        channel.play();
    }
}
