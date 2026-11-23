use super::{
    Analyzer, BoundName, Kind, LookupError, Namespace, QualifiedName, SyntaxDecl, namespace,
};
use crate::{RelatedLocation, TypeReference};
use std::collections::BTreeSet;

impl Analyzer<'_, '_> {
    pub(super) fn bind_topics(&mut self) -> Option<Vec<usize>> {
        let mut topics = Vec::new();
        for id in 0..self.symbols.len() {
            let Some(SyntaxDecl::Topic { options, .. }) = self.symbols[id].ast else {
                continue;
            };
            topics.push(id);
            if let Some(name) = &options.base {
                let (base, reference) = self.topic_reference(id, name)?;
                if self.symbols[base].declaration.is_final() {
                    self.diagnostics.push(
                        "ILI-TOPIC-FINAL",
                        "A FINAL topic cannot be extended",
                        Some(name.span.clone()),
                    );
                    return None;
                }
                self.symbols[id].base_scope = Some(base);
                self.symbols[id].declaration.base_reference = Some(reference);
            }
        }
        let ordered = self.inheritance_order(topics, "ILI-TOPIC-CYCLE")?;
        for &id in &ordered {
            let Some(SyntaxDecl::Topic { options, .. }) = self.symbols[id].ast else {
                unreachable!("only topics have topic dependencies");
            };
            let mut targets = BTreeSet::new();
            let mut dependencies = Vec::new();
            if let Some(base) = self.symbols[id].base_scope {
                let inherited = self.symbols[base].declaration.topic_dependencies();
                let bytes = inherited
                    .iter()
                    .map(|dep| dep.target.len() + dep.declared.len() + 1)
                    .sum();
                let location = self.symbols[id].name_span.clone();
                if !self.step(bytes, &location) {
                    return None;
                }
                for dependency in self.symbols[base].declaration.topic_dependencies() {
                    targets.insert(self.names[dependency.target_name()]);
                    dependencies.push(dependency.clone());
                }
            }
            for name in &options.dependencies {
                let (target, reference) = self.topic_reference(id, name)?;
                if targets.insert(target) {
                    dependencies.push(reference);
                }
            }
            if !dependencies.is_empty() {
                self.symbols[id].declaration.topic_dependencies =
                    Some(dependencies.into_boxed_slice());
            }
        }
        self.check_topic_dependency_cycles(&ordered)?;
        Some(ordered)
    }

    fn topic_reference(
        &mut self,
        owner: usize,
        name: &QualifiedName,
    ) -> Option<(usize, TypeReference)> {
        match self.lookup_name(owner, &name.segments, &name.span, &[Kind::Topic]) {
            Ok(bound @ BoundName::Declared(id)) => {
                Some((id, self.bound_reference(owner, name, bound)?))
            }
            Ok(BoundName::Predefined(..)) => {
                self.diagnostics.push(
                    "ILI-UNSUPPORTED",
                    "Extension or dependency on a predefined topic needs its full model semantics",
                    Some(name.span.clone()),
                );
                None
            }
            Err(LookupError::Limit) => None,
            Err(_) => {
                self.diagnostics.push(
                    "ILI-NAME-NOT-FOUND",
                    "Topic reference cannot be resolved",
                    Some(name.span.clone()),
                );
                None
            }
        }
    }

    fn check_topic_dependency_cycles(&mut self, topics: &[usize]) -> Option<()> {
        #[derive(Clone, Copy, PartialEq)]
        enum Visit {
            Unseen,
            Active,
            Done,
        }
        let mut visited = vec![Visit::Unseen; self.symbols.len()];
        let mut path = Vec::new();
        for &id in topics {
            if visited[id] != Visit::Unseen {
                continue;
            }
            visited[id] = Visit::Active;
            path.push((id, 0));
            while let Some((id, index)) = path.last_mut() {
                let Some(edge) = self.symbols[*id]
                    .declaration
                    .topic_dependencies()
                    .get(*index)
                else {
                    visited[*id] = Visit::Done;
                    path.pop();
                    continue;
                };
                *index += 1;
                let target = self.names[edge.target_name()];
                let location = edge.span.clone();
                if !self.step(1, &location) {
                    return None;
                }
                match visited[target] {
                    Visit::Done => {}
                    Visit::Active => {
                        self.diagnostics.push(
                            "ILI-TOPIC-DEPENDENCY-CYCLE",
                            "Topic dependency closes a cycle",
                            Some(location),
                        );
                        return None;
                    }
                    Visit::Unseen => {
                        visited[target] = Visit::Active;
                        path.push((target, 0));
                    }
                }
            }
        }
        Some(())
    }

    pub(super) fn check_topic_members(&mut self) -> Option<()> {
        for id in 0..self.symbols.len() {
            let declaration = &self.symbols[id].declaration;
            if namespace(declaration.kind) != Namespace::Type {
                continue;
            }
            let Some(parent) = declaration.parent else {
                continue;
            };
            let Some(base) = self.symbols[parent].base_scope else {
                continue;
            };
            let name = declaration.name.clone();
            let location = self.symbols[id].name_span.clone();
            let inherited = self.visible_name(base, &name, None, &location).ok()?;
            if let Some(inherited) = inherited
                && !(self.symbols[id].base_scope == Some(inherited)
                    && matches!(
                        self.symbols[id].ast,
                        Some(SyntaxDecl::Container { base: None, .. })
                    ))
            {
                self.diagnostics.push_related(
                    "ILI-TOPIC-INHERITED-NAME",
                    "A declaration cannot redeclare an inherited topic member",
                    Some(location),
                    vec![RelatedLocation {
                        span: self.symbols[inherited].name_span.clone(),
                        message: "Inherited declaration".into(),
                    }],
                );
                return None;
            }
        }
        Some(())
    }

    pub(super) fn check_abstract_topics(&mut self, topics: &[usize]) -> Option<()> {
        if !self.symbols.iter().any(|symbol| {
            symbol.declaration.kind == Kind::Class && symbol.declaration.is_abstract()
        }) {
            return Some(());
        }
        let mut direct = vec![Vec::new(); self.symbols.len()];
        for (id, symbol) in self.symbols.iter().enumerate() {
            if symbol.declaration.kind == Kind::Class
                && let Some(parent) = symbol.declaration.parent
                && self.symbols[parent].declaration.kind == Kind::Topic
            {
                direct[parent].push(id);
            }
        }
        for &topic in topics {
            let span = self.symbols[topic].name_span.clone();
            if let Some(base) = self.symbols[topic].base_scope {
                if !self.step(direct[base].len(), &span) {
                    return None;
                }
                let [classes, inherited] = direct
                    .get_disjoint_mut([topic, base])
                    .expect("topics are acyclic");
                classes.extend_from_slice(inherited);
            }
            if self.symbols[topic].declaration.is_abstract() {
                continue;
            }
            let mut concrete_bases = BTreeSet::new();
            let classes = &direct[topic];
            for &class in classes {
                if !self.step(1, &span) {
                    return None;
                }
                if self.symbols[class].declaration.is_abstract() {
                    continue;
                }
                let mut base = self.symbols[class].base_scope;
                while let Some(id) = base {
                    if !self.step(1, &span) {
                        return None;
                    }
                    if !concrete_bases.insert(id) {
                        break;
                    }
                    base = self.symbols[id].base_scope;
                }
            }
            if let Some(&class) = classes.iter().find(|&&class| {
                self.symbols[class].declaration.is_abstract() && !concrete_bases.contains(&class)
            }) {
                self.diagnostics.push_related(
                    "ILI-TOPIC-ABSTRACT",
                    "A concrete topic requires a concrete specialization for each abstract class",
                    Some(span),
                    vec![RelatedLocation {
                        span: self.symbols[class].name_span.clone(),
                        message: "Abstract class without concrete specialization".into(),
                    }],
                );
                return None;
            }
        }
        Some(())
    }
}
