use std::collections::HashMap;

use petgraph::prelude::NodeIndex as PetGraphIndex;
use psy_ast::{Location, ModuleId, Position, Visibility, VisitorContext};
use psy_common::FileId;
use psy_vm::dpn::ops::context_trait::ContextFelt;
use rangemap::RangeMap;

use crate::{AstVisualizer, TypeCheckerVisitorContext, TypeId, VarId};

impl<F: Clone + From<u32> + ContextFelt, C> TypeCheckerVisitorContext<F, C> {
    pub fn add_type_reference(&mut self, type_id: TypeId, location: Location, is_self_type: bool) {
        if type_id.is_std_type() {
            return;
        }
        let referenced = ReferenceId::Type(type_id);
        self.add_reference(referenced, location, is_self_type);
    }

    pub fn add_variable_reference(&mut self, variable_id: VarId, location: Location, is_self_type: bool) {
        let referenced = ReferenceId::Variable(variable_id);
        self.add_reference(referenced, location, is_self_type);
    }

    pub fn add_module_reference(&mut self, module_id: ModuleId, location: Location, is_self_type: bool) {
        let referenced = ReferenceId::Module(module_id);
        self.add_reference(referenced, location, is_self_type);
    }

    pub fn add_reference(&mut self, referenced: ReferenceId, location: Location, is_self_type: bool) {
        let reference = ReferenceId::Reference(location, is_self_type);

        let referenced_index = self.get_or_insert_reference(referenced);
        let reference_location = self.reference_location(reference);
        let reference_index = self.reference_graph.add_node(reference);

        self.reference_graph.add_edge(reference_index, referenced_index, ());
        self.location_indices.insert_span(reference_location, reference_index);
    }

    pub fn find_all_references(&self, location: Location, include_referenced: bool, include_self_type_name: bool) -> Option<Vec<Location>> {
        let referenced_node = self.find_referenced(location)?;
        let referenced_node_index = self.reference_graph_indices[&referenced_node];

        let found_locations = self.find_all_references_for_index(referenced_node_index, include_referenced, include_self_type_name);

        Some(found_locations)
    }

    pub fn goto_definition(&self, location: Location) -> Option<Location> {
        let referenced_node = self.find_referenced(location)?;
        Some(self.reference_location(referenced_node))
    }

    pub fn hover(&self, location: Location) -> Option<String> {
        let referenced_node = self.find_referenced(location)?;
        match referenced_node {
            ReferenceId::Reference(_location, _is_self) => {
                // ?
                unreachable!()
            }
            ReferenceId::Module(module_id) => {
                // module absolute path
                let module = &self.symbols[module_id];
                let module_detail = format!(
                    "{}mod {}",
                    if module.visibility == Visibility::Public { "pub " } else { "" },
                    self.ident(module.name)
                );
                Some(module_detail)
            }
            ReferenceId::Type(type_id) => {
                // type definition
                let type_detail = format!("{}", self.debug_type(type_id));
                Some(type_detail)
            }
            ReferenceId::Variable(var_id) => {
                // `let variable_name: type_name` or `variable_name: type_name`
                let variable = &self.symbols[var_id];
                // let variable_ty = &self.symbols[variable.ty];
                let variable_detail = format!("{} : {}", self.ident(variable.name), self.debug_type(variable.ty));
                Some(variable_detail)
            }
        }
    }

    pub(crate) fn get_or_insert_reference(&mut self, id: ReferenceId) -> PetGraphIndex {
        if let Some(index) = self.reference_graph_indices.get(&id) {
            return *index;
        }

        let index = self.reference_graph.add_node(id);
        self.reference_graph_indices.insert(id, index);
        index
    }

    pub fn reference_location(&self, reference: ReferenceId) -> Location {
        match reference {
            ReferenceId::Reference(location, _) => location,
            ReferenceId::Module(module_id) => self.symbols[module_id].location,
            ReferenceId::Type(type_id) => self.symbols[type_id].location(),
            ReferenceId::Variable(var_id) => self.symbols[var_id].location,
        }
    }

    pub fn find_referenced(&self, location: Location) -> Option<ReferenceId> {
        let node_index = self.location_indices.resolve_node_at(location)?;
        let reference_node = self.reference_graph[node_index];

        if let ReferenceId::Reference(_, _) = reference_node {
            let node_index = self.referenced_index(node_index)?;
            Some(self.reference_graph[node_index])
        } else {
            Some(reference_node)
        }
    }

    fn referenced_index(&self, reference_index: PetGraphIndex) -> Option<PetGraphIndex> {
        self.reference_graph
            .neighbors_directed(reference_index, petgraph::Direction::Outgoing)
            .next()
    }

    fn find_all_references_for_index(
        &self,
        referenced_node_index: PetGraphIndex,
        include_referenced: bool,
        include_self_type_name: bool,
    ) -> Vec<Location> {
        let id = self.reference_graph[referenced_node_index];
        let mut edit_locations = Vec::new();
        if include_referenced && (include_self_type_name || !id.is_self_type_name()) {
            edit_locations.push(self.reference_location(id));
        }

        self.reference_graph
            .neighbors_directed(referenced_node_index, petgraph::Direction::Incoming)
            .for_each(|reference_node_index| {
                let id = self.reference_graph[reference_node_index];
                if include_self_type_name || !id.is_self_type_name() {
                    edit_locations.push(self.reference_location(id));
                }
            });
        edit_locations
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ReferenceId {
    Module(ModuleId),
    Type(TypeId),
    Variable(VarId),
    Reference(Location, bool /* is Self */),
}

impl ReferenceId {
    pub fn is_self_type_name(&self) -> bool {
        matches!(self, Self::Reference(_, true))
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LocationIndices {
    map_file_to_range: HashMap<FileId, RangeMap<usize, PetGraphIndex>>,
}

impl LocationIndices {
    /// Insert a node's source code span into the file-specific range map.
    pub(crate) fn insert_span(&mut self, location: Location, node_index: PetGraphIndex) {
        // Skip empty spans, which may come from synthetic or placeholder nodes.
        if location.start == location.end {
            return;
        }

        let range_map = self.map_file_to_range.entry(location.file_id).or_default();
        range_map.insert(location.start..location.end, node_index);
    }

    /// Find the graph node index corresponding to a given source location.
    ///
    /// This is typically used during `goto definition` or `find references`.
    /// It tolerates off-by-one errors caused by cursor positions being placed
    /// immediately after the end of a word (e.g., clicking after an
    /// identifier).
    pub(crate) fn resolve_node_at(&self, location: Location) -> Option<PetGraphIndex> {
        // Retrieve the range map for the given file.
        let range_table = self.map_file_to_range.get(&location.file_id)?;

        // Try exact match on the starting byte offset.
        if let Some(index) = range_table.get(&location.start) {
            return Some(*index);
        }

        // Fault-tolerance: if the cursor is just after a valid token (e.g., end of a
        // word), try matching the previous byte.
        if location.start > 0 {
            if let Some(index) = range_table.get(&(location.start - 1)) {
                return Some(*index);
            }
        }

        None
    }
}

impl<F: Clone + From<u32> + ContextFelt, C> TypeCheckerVisitorContext<F, C> {
    pub fn location_to_position(&self, location: Location) -> Option<(Position, Position)> {
        let file_content = self.program.file_resolver.resolve_content(&location.file_id)?;
        let (start_line, start_column) = line_and_column_from_offset(&file_content, location.start);
        let (end_line, end_column) = line_and_column_from_offset(&file_content, location.end);
        Some((
            Position {
                file_id: location.file_id,
                line: start_line,
                column: start_column,
            },
            Position {
                file_id: location.file_id,
                line: end_line,
                column: end_column,
            },
        ))
    }

    pub fn position_to_location(&self, position: Position) -> Option<Location> {
        let file_content = self.program.file_resolver.resolve_content(&position.file_id)?;
        let offset = offset_from_position(&file_content, &position);
        Some(Location {
            file_id: position.file_id,
            start: offset,
            end: offset + 1,
        })
    }

    pub fn position_to_file_path(&self, position: Position) -> Option<String> {
        format!(
            "{}:{}:{}",
            self.program.file_resolver.resolve_path(&position.file_id)?.display(),
            position.line,
            position.column
        )
        .into()
    }
}

pub fn line_and_column_from_offset(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 0;

    for (i, char) in source.chars().enumerate() {
        column += 1;

        if char == '\n' {
            line += 1;
            column = 0;
        }

        if offset <= i {
            break;
        }
    }

    (line, column)
}

pub fn offset_from_position(source: &str, position: &Position) -> usize {
    let mut offset = 0;
    let lines = source.lines().collect::<Vec<&str>>();

    // Check if the line number is out of line
    if position.line as usize >= lines.len() {
        return source.len();
    }

    for i in 0..position.line as usize {
        offset += lines[i].len() + 1; // +1 for '\n'
    }

    let current_line = lines[position.line as usize];
    if position.column as usize >= current_line.len() {
        offset += current_line.len();
    } else {
        offset += position.column;
    }

    offset
}
