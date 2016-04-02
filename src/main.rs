extern crate clap;
extern crate cpal;
extern crate rand;
extern crate regex;
extern crate vorbis;

use clap::{Arg, App};
use rand::Rng;
use regex::Regex;
use std::error::Error;
use std::fs::File;
use std::io::{Write, stderr};
use std::num::ParseIntError;
use std::path::{Path, PathBuf};
use std::process;
use std::str::FromStr;
use std::thread;
use std::time;

enum MyError {
    ParseInt(ParseIntError),
    Vorbis(vorbis::VorbisError),
    Composite(Vec<MyError>),
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

trait Stream {
    fn size_hint(&self) -> (usize, Option<usize>);
    fn next_slice(&mut self, usize) -> Result<&[f32], MyError>;
    fn get_tail(&mut self) -> Option<Result<Box<Stream>, MyError>>;
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
                    recycled.extend(data.iter().map(|value| *value as f32));
                    self.next_packet = Some(recycled);
                }
            }
        }
        let (min, _) = self.size_hint();
        if size > min {
            panic!("out of bounds");
        }
        let old_offset = self.offset;
        self.offset += size;
        Ok(&self.packet[old_offset..self.offset])
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let min = if self.offset == self.packet.len() {
            if let Some(ref packet) = self.next_packet {
                packet.len()
            } else {
                0
            }
        } else {
            self.packet.len() - self.offset
        };
        let max = if self.next_packet.is_none() {
            Some(min)
        } else {
            None
        };
        (min, max)
    }

    fn get_tail(&mut self) -> Option<Result<Box<Stream>, MyError>> {
        None
    }
}

struct EmptyStream([f32; 0]);

impl Stream for EmptyStream {
    fn next_slice(&mut self, size: usize) -> Result<&[f32], MyError> {
        if size == 0 {
            Ok(&self.0)
        } else {
            panic!("out of bounds");
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(0))
    }
    fn get_tail(&mut self) -> Option<Result<Box<Stream>, MyError>> {
        None
    }
}

struct Track {
    stream: Box<Stream>,
    splice_point: Option<u64>,
}

impl Track {
    fn into_stream(self) -> Box<Stream> {
        let (_, max) = self.size_hint();
        if max.map(|max| max == 0).unwrap_or(false) {
            self.stream
        } else {
            panic!("unconsumed data");
        }
    }
}

impl Stream for Track {
    fn next_slice(&mut self, size: usize) -> Result<&[f32], MyError> {
        let (min, _) = self.size_hint();
        if size > min {
            panic!("out of bounds");
        }
        self.splice_point = self.splice_point.map(|sp| sp - size as u64);
        self.stream.next_slice(size)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (min, max) = self.stream.size_hint();
        self.splice_point
            .map(|sp| {
                (std::cmp::min(sp, min as u64) as usize,
                 max.map(|max| std::cmp::min(sp, max as u64) as usize).or_else(|| {
                    if sp <= usize::max_value() as u64 {
                        Some(sp as usize)
                    } else {
                        None
                    }
                }))
            })
            .unwrap_or((min, max))
    }

    fn get_tail(&mut self) -> Option<Result<Box<Stream>, MyError>> {
        let (_, max) = self.size_hint();
        if max.map(|max| max == 0).unwrap_or(false) {
            let tail = std::mem::replace(&mut self.stream, Box::new(EmptyStream([])));
            if tail.size_hint().0 > 0 {
                Some(Ok(tail))
            } else {
                None
            }
        } else {
            panic!("unconsumed data");
        }
    }
}

struct Digraph(Vec<Vec<(usize, Vec<PathBuf>)>>);

impl Digraph {
    fn random_walk<'a>(&'a self, state: usize, rng: Box<Rng>) -> RandomWalk<'a> {
        RandomWalk {
            state: state,
            digraph: &self,
            rng: rng,
        }
    }
}

struct RandomWalk<'a> {
    state: usize,
    digraph: &'a Digraph,
    rng: Box<Rng>,
}

impl<'a> Iterator for RandomWalk<'a> {
    type Item = &'a Path;
    fn next(&mut self) -> Option<&'a Path> {
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
    let stream = VorbisStream {
        offset: 0,
        packet: vec![],
        next_packet: Some(vec![]),
        packets: decoder.into_packets(),
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

impl Stream for Player {
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.track.size_hint()
    }
    fn next_slice(&mut self, size: usize) -> Result<&[f32], MyError> {
        self.track.next_slice(size)
    }
    fn get_tail(&mut self) -> Option<Result<Box<Stream>, MyError>> {
        self.track.get_tail().map(|tail| {
            if self.track.size_hint().0 == 0 {
                match self.play_list.next() {
                    Some(Ok(track)) => {
                        self.track = track;
                    }
                    Some(Err(track)) => {}
                    None => (),
                }
            }
            tail
        })
    }
}

struct Mixer {
    coefficient: f32,
    streams: Vec<Box<Stream>>,
}

impl Mixer {
    fn mix_next_slice(&mut self, buf: &mut [f32]) -> Result<(), MyError> {
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
    fn each_next_slice(&mut self, size: usize, f: &mut FnMut(&[f32])) -> Result<(), MyError> {
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
            if stream.size_hint().0 == 0 {
                match stream.get_tail() {
                    Some(Ok(tail)) => {
                        new_tails.push(tail);
                    }
                    Some(Err(err)) => {
                        errors.push(err);
                    }
                    None => (),
                }
                if let Some(tail) = stream.get_tail() {
                } else if stream.size_hint().0 == 0 {
                    empties.push(i);
                }
            }
        }
        empties.reverse();
        for i in empties {
            self.streams.swap_remove(i);
        }
        self.streams.extend(new_tails);

        if errors.len() == 0 {
            Ok(())
        } else {
            Err(MyError::Composite(errors))
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.streams
            .iter()
            .fold(None as Option<(usize, Option<usize>)>, |acc, stream| {
                let (min, max) = stream.size_hint();
                acc.map(|acc| {
                       let (acc_min, acc_max) = acc;
                       (std::cmp::min(acc_min, min),
                        max.map(|max| {
                               acc_max.map(|acc_max| std::cmp::min(acc_max, max))
                                      .unwrap_or(max)
                           })
                           .or(acc_max))
                   })
                   .or(Some((min, max)))
            })
            .unwrap_or((0, Some(0)))
    }
}

fn main() {}
