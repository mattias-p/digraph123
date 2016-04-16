use rand::Rng;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Digraph(Vec<Vec<(usize, Vec<PathBuf>)>>);

impl Digraph {
    pub fn into_random_walk(self, rng: Box<Rng>) -> IntoRandomWalk {
        IntoRandomWalk {
            state: 0,
            digraph: self,
            rng: rng,
        }
    }
}

pub struct DigraphBuilder {
    indices: HashMap<String, usize>,
    arrows: HashMap<(usize, usize), Vec<PathBuf>>,
}

impl DigraphBuilder {
    pub fn new() -> DigraphBuilder {
        let mut indices = HashMap::new();
        indices.insert("start".to_string(), 0);
        DigraphBuilder {
            indices: indices,
            arrows: HashMap::new(),
        }
    }
    pub fn arrow(mut self, tail: String, head: String, path: PathBuf) -> Self {
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

pub struct IntoRandomWalk {
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
