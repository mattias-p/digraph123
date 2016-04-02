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

impl Player {
    fn next_track(&mut self) -> Option<Result<Box<Stream>, MyError>> {
        self.play_list.next().map(|track| {
            let new_track = try!(track);
            let old_track = std::mem::replace(&mut self.track, new_track);
            Ok(old_track.into_stream())
        })
    }
}

impl Stream for Player {
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.track.size_hint()
    }
    fn next_slice(&mut self, size: usize) -> Result<&[f32], MyError> {
        self.track.next_slice(size)
    }
}


trait MultiStream {
    fn each_next_slice(&mut self, size: usize, f: &mut FnMut(&[f32])) -> Result<(), MyError>;
    fn size_hint(&self) -> (usize, Option<usize>);
}

struct SimpleStream(Box<Stream>);

impl MultiStream for SimpleStream {
    fn each_next_slice(&mut self, size: usize, f: &mut FnMut(&[f32])) -> Result<(), MyError> {
        f(try!(self.0.next_slice(size)));
        Ok(())
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

struct Ensamble(Vec<Box<MultiStream>>);

impl Ensamble {
    fn mix_next_slice(&mut self, buf: &mut [f32], c: f32) -> Result<(), MyError> {
        let c = 1.0 / self.0.len() as f32;
        let size = buf.len();
        for out in buf.iter_mut() {
            *out = 0.0;
        }
        self.each_next_slice(size,
                             &mut |slice| {
                                 for (out, value) in buf.iter_mut().zip(slice) {
                                     *out += c * *value;
                                 }
                             })
    }
}

impl MultiStream for Ensamble {
    fn each_next_slice(&mut self, size: usize, f: &mut FnMut(&[f32])) -> Result<(), MyError> {
        let mut errors = vec![];
        for multi_stream in self.0.iter_mut() {
            match multi_stream.as_mut().each_next_slice(size, f) {
                Err(err) => errors.push(err),
                _ => (),
            };
        }
        if errors.len() == 0 {
            Ok(())
        } else {
            Err(MyError::Composite(errors))
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0
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
