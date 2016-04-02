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
use std::path::Path;
use std::process;
use std::time;
use std::thread;
use std::collections::HashMap;

enum MyError {
    Vorbis(vorbis::VorbisError),
    Composite(Vec<MyError>),
}

impl From<vorbis::VorbisError> for MyError {
    fn from(err: vorbis::VorbisError) -> MyError {
        MyError::Vorbis(err)
    }
}

trait Stream {
    fn size_hint(&self) -> (usize, Option<usize>);
    fn next_slice(&mut self, usize) -> Result<&[i16], MyError>;
}

pub struct VorbisStream {
    offset: usize,
    packet: Vec<i16>,
    next_packet: Option<Vec<i16>>,
    packets: vorbis::PacketsIntoIter<File>,
}

impl Stream for VorbisStream {
    fn next_slice(&mut self, size: usize) -> Result<&[i16], MyError> {
        if self.offset == self.packet.len() {
            let next_packet = if let Some(next_packet) = self.packets.next() {
                Some(try!(next_packet).data)
            } else {
                None
            };
            if let Some(packet) = std::mem::replace(&mut self.next_packet, next_packet) {
                self.packet = packet;
                self.offset = 0;
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
    fn next_slice(&mut self, size: usize) -> Result<&[i16], MyError> {
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

trait MultiStream {
    fn each_next_slice(&mut self, size: usize, f: &mut FnMut(&[i16])) -> Result<(), MyError>;
    fn size_hint(&self) -> (usize, Option<usize>);
}

struct SimpleStream(Box<Stream>);

impl MultiStream for SimpleStream {
    fn each_next_slice(&mut self, size: usize, f: &mut FnMut(&[i16])) -> Result<(), MyError> {
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
        let c = (self.0.len() as f32) / (i16::max_value() as f32);
        let size = buf.len();
        for out in buf.iter_mut() {
            *out = 0.0;
        }
        self.each_next_slice(size,
                             &mut |slice| {
                                 for (out, value) in buf.iter_mut().zip(slice) {
                                     *out += (*value as f32) * c;
                                 }
                             })
    }
}

impl MultiStream for Ensamble {
    fn each_next_slice(&mut self, size: usize, f: &mut FnMut(&[i16])) -> Result<(), MyError> {
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
