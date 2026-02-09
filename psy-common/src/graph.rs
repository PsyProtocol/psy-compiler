use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::Hash,
    ops::Deref,
};

use indexmap::IndexSet;

use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    #[allow(dead_code)]
    White,
    Grey,
    Black,
}

#[derive(Clone, Debug)]
pub struct Graph<T> {
    edges: HashMap<T, IndexSet<T>>,
}

impl<T: Clone + Eq + Hash> Graph<T> {
    pub fn new() -> Self {
        Self { edges: HashMap::new() }
    }

    pub fn add_node(&mut self, node: T) {
        self.edges.entry(node).or_default();
    }

    pub fn add_edge(&mut self, from: T, to: T) {
        if from != to {
            self.edges.entry(from).or_default().insert(to.clone());
        }
        self.edges.entry(to).or_default();
    }

    pub fn nodes(&self) -> Vec<&T> {
        self.edges.keys().collect()
    }

    pub fn edges(&self, node: &T) -> Option<&IndexSet<T>> {
        self.edges.get(node)
    }

    pub fn contains_node(&self, node: &T) -> bool {
        self.edges.contains_key(node)
    }

    pub fn starting_nodes(&self) -> Vec<&T> {
        let mut starting_nodes = self.edges.keys().collect::<HashSet<_>>();
        for node in self.edges.keys() {
            if let Some(neighbors) = self.edges.get(node) {
                for neighbor in neighbors {
                    starting_nodes.remove(neighbor);
                }
            }
        }
        starting_nodes.into_iter().collect()
    }

    pub fn dfs<'a>(&'a self, visitor: &mut impl FnMut(&'a T, Option<&'a T>)) {
        let starting_nodes = self.starting_nodes();
        for node in starting_nodes {
            self.dfs_inner(node, None, visitor);
        }
    }

    fn dfs_inner<'a>(&'a self, node: &'a T, parent: Option<&'a T>, visitor: &mut impl FnMut(&'a T, Option<&'a T>)) {
        visitor(node, parent);

        if let Some(neighbors) = self.edges.get(&node) {
            for neighbor in neighbors {
                self.dfs_inner(neighbor, Some(node), visitor);
            }
        }
    }

    pub fn bfs<'a, E: From<Error>>(&'a self, visitor: &mut impl FnMut(&'a T) -> Result<(), E>) -> Result<(), E> {
        let starting_nodes = self.starting_nodes();
        let mut visited = HashMap::new();
        for node in starting_nodes {
            self.bfs_inner(node, &mut visited, visitor)?;
        }
        Ok(())
    }

    fn bfs_inner<'a, E: From<Error>>(
        &'a self,
        node: &'a T,
        visited: &mut HashMap<&'a T, bool>,
        visitor: &mut impl FnMut(&'a T) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut queue = VecDeque::new();
        queue.push_back(node);
        visited.insert(node, true);

        while let Some(node) = queue.pop_front() {
            visitor(node)?;

            if let Some(neighbors) = self.edges.get(node) {
                for neighbor in neighbors {
                    if !visited.contains_key(neighbor) {
                        visited.insert(neighbor, true);
                        queue.push_back(neighbor);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn ts<'a, E: From<Error>>(&'a self, visitor: &mut impl FnMut(&'a T) -> Result<(), E>) -> Result<(), E> {
        let starting_nodes = self.starting_nodes();
        let mut colors = HashMap::new();
        for node in starting_nodes {
            self.ts_inner(node, &mut colors, visitor)?;
        }
        Ok(())
    }

    fn ts_inner<'a, E: From<Error>>(
        &'a self,
        node: &'a T,
        colors: &mut HashMap<&'a T, Color>,
        visitor: &mut impl FnMut(&'a T) -> Result<(), E>,
    ) -> Result<(), E> {
        colors.insert(node, Color::Grey);

        if let Some(neighbors) = self.edges.get(&node) {
            for neighbor in neighbors {
                match colors.get(neighbor) {
                    Some(Color::Grey) => return Err(E::from(Error::CycleGraph)),
                    None => {
                        self.ts_inner(neighbor, colors, visitor)?;
                    }
                    _ => {}
                }
            }
        }

        visitor(node)?;
        colors.insert(node, Color::Black);

        Ok(())
    }

    pub fn check_cycle<E: From<Error>>(&self) -> Result<(), E> {
        self.ts::<E>(&mut |_| Ok(()))
    }
}
