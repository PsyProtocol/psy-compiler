use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::Hash,
};

use indexmap::{IndexMap, IndexSet};

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
    edges: IndexMap<T, IndexSet<T>>,
}

impl<T: Clone + Eq + Hash> Graph<T> {
    pub fn new() -> Self {
        Self { edges: IndexMap::new() }
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
        let mut starting_nodes = self.edges.keys().collect::<IndexSet<_>>();
        for node in self.edges.keys() {
            if let Some(neighbors) = self.edges.get(node) {
                for neighbor in neighbors {
                    starting_nodes.shift_remove(neighbor);
                }
            }
        }
        starting_nodes.into_iter().collect()
    }

    pub fn dfs<'a>(&'a self, visitor: &mut impl FnMut(&'a T, Option<&'a T>)) {
        let mut visited = HashSet::new();
        // Iterate every node, rather than only roots: a cyclic component has
        // no starting node and was previously skipped entirely.
        for node in self.edges.keys() {
            self.dfs_inner(node, None, &mut visited, visitor);
        }
    }

    fn dfs_inner<'a>(&'a self, node: &'a T, parent: Option<&'a T>, visited: &mut HashSet<&'a T>, visitor: &mut impl FnMut(&'a T, Option<&'a T>)) {
        if !visited.insert(node) {
            return;
        }
        visitor(node, parent);

        if let Some(neighbors) = self.edges.get(node) {
            for neighbor in neighbors {
                self.dfs_inner(neighbor, Some(node), visited, visitor);
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
        let mut colors = HashMap::new();
        for node in self.edges.keys() {
            if !colors.contains_key(node) {
                self.ts_inner(node, &mut colors, visitor)?;
            }
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

        if let Some(neighbors) = self.edges.get(node) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cycle_without_starting_nodes() {
        let mut graph = Graph::new();
        graph.add_edge("A", "B");
        graph.add_edge("B", "A");

        let result: Result<(), Error> = graph.check_cycle();

        assert!(matches!(result, Err(Error::CycleGraph)));
    }

    #[test]
    fn checks_disconnected_components() {
        let mut graph = Graph::new();
        graph.add_edge("A", "B");
        graph.add_edge("C", "D");

        let result: Result<(), Error> = graph.check_cycle();

        assert!(result.is_ok());
    }

    #[test]
    fn traversal_order_follows_insertion_order() {
        let mut graph = Graph::new();
        graph.add_edge("A", "B");
        graph.add_edge("C", "D");

        assert_eq!(graph.starting_nodes(), vec![&"A", &"C"]);

        let mut visited = Vec::new();
        graph.ts::<Error>(&mut |node| {
            visited.push(*node);
            Ok(())
        }).unwrap();

        assert_eq!(visited, vec!["B", "A", "D", "C"]);
    }

    #[test]
    fn detects_cycle_in_disconnected_component() {
        let mut graph = Graph::new();
        graph.add_edge("root", "leaf");
        graph.add_edge("cycle-a", "cycle-b");
        graph.add_edge("cycle-b", "cycle-a");

        let result: Result<(), Error> = graph.check_cycle();

        assert!(matches!(result, Err(Error::CycleGraph)));
    }

    #[test]
    fn visits_isolated_nodes_and_dependency_nodes_once() {
        let mut graph = Graph::new();
        graph.add_node("isolated");
        graph.add_edge("root-a", "shared");
        graph.add_edge("root-b", "shared");

        let mut visited = Vec::new();
        graph.ts::<Error>(&mut |node| {
            visited.push(*node);
            Ok(())
        }).unwrap();

        assert_eq!(visited, vec!["isolated", "shared", "root-a", "root-b"]);
    }

    #[test]
    fn dfs_visits_cycles_once_instead_of_recursing_forever() {
        let mut graph = Graph::new();
        graph.add_edge("A", "B");
        graph.add_edge("B", "A");

        let mut visited = Vec::new();
        graph.dfs(&mut |node, parent| visited.push((*node, parent.copied())));

        assert_eq!(visited, vec![("A", None), ("B", Some("A"))]);
    }

    #[test]
    fn dfs_visits_shared_dag_node_once() {
        let mut graph = Graph::new();
        graph.add_edge("root", "left");
        graph.add_edge("root", "right");
        graph.add_edge("left", "shared");
        graph.add_edge("right", "shared");

        let mut visited = Vec::new();
        graph.dfs(&mut |node, _| visited.push(*node));

        assert_eq!(visited, vec!["root", "left", "shared", "right"]);
    }

    #[test]
    fn empty_graph_has_no_roots_and_all_traversals_are_noops() {
        let graph = Graph::<&str>::new();
        let mut dfs_count = 0;
        let mut bfs_count = 0;

        graph.dfs(&mut |_, _| dfs_count += 1);
        graph.bfs::<Error>(&mut |_| {
            bfs_count += 1;
            Ok(())
        }).unwrap();

        assert!(graph.nodes().is_empty());
        assert!(graph.starting_nodes().is_empty());
        assert_eq!((dfs_count, bfs_count), (0, 0));
    }

    #[test]
    fn self_edges_are_ignored_and_duplicate_edges_are_deduplicated() {
        let mut graph = Graph::new();
        graph.add_edge("A", "A");
        graph.add_edge("A", "B");
        graph.add_edge("A", "B");

        assert_eq!(graph.nodes(), vec![&"A", &"B"]);
        assert_eq!(graph.edges(&"A").unwrap().iter().collect::<Vec<_>>(), vec![&"B"]);
        assert!(graph.edges(&"B").unwrap().is_empty());
        assert_eq!(graph.starting_nodes(), vec![&"A"]);
    }

    #[test]
    fn bfs_visits_a_shared_node_once_and_propagates_visitor_errors() {
        let mut graph = Graph::new();
        graph.add_edge("root-a", "shared");
        graph.add_edge("root-b", "shared");

        let mut visited = Vec::new();
        let result = graph.bfs::<Error>(&mut |node| {
            visited.push(*node);
            if *node == "root-b" {
                return Err(Error::CycleGraph);
            }
            Ok(())
        });

        assert!(matches!(result, Err(Error::CycleGraph)));
        assert_eq!(visited, vec!["root-a", "shared", "root-b"]);
    }

    #[test]
    fn contains_node_and_edges_reflect_insertions_and_misses() {
        let mut graph = Graph::new();
        graph.add_edge("A", "B");

        assert!(graph.contains_node(&"A"));
        assert!(graph.contains_node(&"B"));
        assert!(!graph.contains_node(&"C"));
        assert_eq!(graph.edges(&"C"), None);
    }

    #[test]
    fn add_node_is_idempotent_for_existing_nodes() {
        let mut graph = Graph::new();
        graph.add_edge("A", "B");
        graph.add_node("A");
        graph.add_node("A");

        assert_eq!(graph.nodes(), vec![&"A", &"B"]);
        assert_eq!(graph.edges(&"A").unwrap().iter().collect::<Vec<_>>(), vec![&"B"]);
    }

    #[test]
    fn topological_sort_propagates_visitor_errors() {
        let mut graph = Graph::new();
        graph.add_edge("A", "B");

        let mut visited = Vec::new();
        let result = graph.ts::<Error>(&mut |node| {
            visited.push(*node);
            if *node == "B" {
                return Err(Error::CycleGraph);
            }
            Ok(())
        });

        assert!(matches!(result, Err(Error::CycleGraph)));
        assert_eq!(visited, vec!["B"]);
    }

    #[test]
    fn bfs_skips_pure_cycles_because_they_have_no_starting_node() {
        // Unlike `dfs`, which iterates every node, `bfs` only walks from
        // `starting_nodes()`. A component that is entirely cyclic therefore
        // contributes no starting node and is silently skipped.
        let mut graph = Graph::new();
        graph.add_edge("A", "B");
        graph.add_edge("B", "A");
        graph.add_edge("root", "A");

        let mut visited = Vec::new();
        graph.bfs::<Error>(&mut |node| {
            visited.push(*node);
            Ok(())
        }).unwrap();

        assert_eq!(visited, vec!["root", "A", "B"]);
    }
}
