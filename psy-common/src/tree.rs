use std::{
    hash::Hash,
    ops::{Index, IndexMut},
};

use crate::{Arena, Graph};

#[derive(Debug, Clone, PartialEq)]
pub struct TreeNode<I: From<usize> + Into<usize> + Copy, T> {
    id: I,
    parent: Option<I>,
    children: Vec<I>,
    data: T,
}

impl<I: From<usize> + Into<usize> + Copy, T> TreeNode<I, T> {
    pub fn new(id: I, data: T) -> Self {
        Self {
            id,
            parent: None,
            children: Vec::new(),
            data,
        }
    }

    pub fn id(&self) -> I {
        self.id
    }

    pub fn parent(&self) -> Option<I> {
        self.parent
    }

    pub fn children(&self) -> &[I] {
        &self.children
    }

    pub fn children_mut(&mut self) -> &mut Vec<I> {
        &mut self.children
    }

    pub fn data(&self) -> &T {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

#[derive(Debug, Clone)]
pub struct Tree<I: From<usize> + Into<usize> + Copy, T> {
    nodes: Arena<I, TreeNode<I, T>>,
}

impl<I: From<usize> + Into<usize> + Copy, T> Tree<I, T> {
    pub fn new() -> Self {
        Self { nodes: Arena::new() }
    }
}

impl<I: From<usize> + Into<usize> + Copy, T> Tree<I, T> {
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn next_idx(&mut self) -> I {
        self.nodes.next_idx()
    }

    pub fn nodes(&self) -> &Arena<I, TreeNode<I, T>> {
        &self.nodes
    }

    pub fn add_node(&mut self, data: T) -> I {
        let id = self.nodes.len().into();
        let node = TreeNode::new(id, data);
        self.nodes.alloc_item(node)
    }

    pub fn add_child(&mut self, parent_id: I, child_id: I) -> I {
        self.nodes[child_id].parent = Some(parent_id);
        self.nodes[parent_id].children.push(child_id);
        child_id
    }

    pub fn dfs(&self, node: I, visitor: &mut impl FnMut(&TreeNode<I, T>)) {
        visitor(&self.nodes[node]);
        for &child in self.nodes[node].children() {
            self.dfs(child, visitor);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &TreeNode<I, T>> {
        self.nodes.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut TreeNode<I, T>> {
        self.nodes.iter_mut()
    }

    pub fn to_graph(&self) -> Graph<I>
    where
        I: Eq + Hash,
    {
        let mut graph = Graph::new();
        for node in self.iter() {
            for &child in node.children() {
                graph.add_edge(node.id(), child);
            }
        }
        graph
    }
}

impl<I, T> Index<I> for Tree<I, T>
where
    I: From<usize> + Into<usize> + Copy,
{
    type Output = TreeNode<I, T>;
    fn index(&self, index: I) -> &Self::Output {
        &self.nodes[index]
    }
}

impl<I, T> IndexMut<I> for Tree<I, T>
where
    I: From<usize> + Into<usize> + Copy,
{
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        &mut self.nodes[index]
    }
}
