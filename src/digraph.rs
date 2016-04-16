use rand;
use rand::Rng;
use std::collections;
use std::path;

pub struct Digraph(Vec<Vec<(usize, Vec<path::PathBuf>)>>);

impl Digraph {
    pub fn into_random_walk(self, rng: Box<rand::Rng>) -> IntoRandomWalk {
        IntoRandomWalk {
            state: 0,
            digraph: self,
            rng: rng,
        }
    }
}

pub struct DigraphBuilder {
    indices: collections::HashMap<String, usize>,
    arrows: collections::HashMap<(usize, usize), Vec<path::PathBuf>>,
}

impl DigraphBuilder {
    pub fn new() -> DigraphBuilder {
        let mut indices = collections::HashMap::new();
        indices.insert("start".to_string(), 0);
        DigraphBuilder {
            indices: indices,
            arrows: collections::HashMap::new(),
        }
    }
    pub fn arrow(mut self, tail: String, head: String, path: path::PathBuf) -> Self {
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
    rng: Box<rand::Rng>,
}

impl IntoRandomWalk {
    fn next_once(&mut self) -> Option<&path::Path> {
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
    type Item = path::PathBuf;
    fn next(&mut self) -> Option<path::PathBuf> {
        let path = self.next_once().map(|p| p.to_path_buf());
        path.or_else(|| self.next_once().map(|p| p.to_path_buf()))
    }
}
