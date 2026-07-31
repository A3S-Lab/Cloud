use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{NodeKind, WorkflowEdge, WorkflowError, WorkflowNode, WorkflowResult};

pub fn topological_order(
    nodes: &[WorkflowNode],
    edges: &[WorkflowEdge],
) -> WorkflowResult<Vec<String>> {
    if nodes.is_empty() {
        return Err(WorkflowError::Validation(
            "a workflow must contain nodes".to_string(),
        ));
    }

    let mut node_by_id = BTreeMap::new();
    let mut start_nodes = Vec::new();
    let mut output_nodes = Vec::new();
    for node in nodes {
        validate_identifier("node", &node.id)?;
        if node.data.label.trim().is_empty() {
            return Err(WorkflowError::Validation(format!(
                "node {} must have a label",
                node.id
            )));
        }
        if node_by_id.insert(node.id.clone(), node).is_some() {
            return Err(WorkflowError::Validation(format!(
                "duplicate node id {}",
                node.id
            )));
        }
        match node.kind {
            NodeKind::Start => start_nodes.push(node.id.clone()),
            NodeKind::Output => output_nodes.push(node.id.clone()),
            _ => {}
        }
    }

    if start_nodes.len() != 1 {
        return Err(WorkflowError::Validation(format!(
            "a workflow must contain exactly one start node, found {}",
            start_nodes.len()
        )));
    }
    if output_nodes.len() != 1 {
        return Err(WorkflowError::Validation(format!(
            "a workflow must contain exactly one output node, found {}",
            output_nodes.len()
        )));
    }

    let mut edge_ids = BTreeSet::new();
    let mut outgoing: BTreeMap<String, Vec<String>> = node_by_id
        .keys()
        .cloned()
        .map(|id| (id, Vec::new()))
        .collect();
    let mut incoming: BTreeMap<String, Vec<String>> = node_by_id
        .keys()
        .cloned()
        .map(|id| (id, Vec::new()))
        .collect();

    for edge in edges {
        validate_identifier("edge", &edge.id)?;
        if !edge_ids.insert(edge.id.clone()) {
            return Err(WorkflowError::Validation(format!(
                "duplicate edge id {}",
                edge.id
            )));
        }
        if edge.source == edge.target {
            return Err(WorkflowError::Validation(format!(
                "edge {} cannot connect a node to itself",
                edge.id
            )));
        }
        if !node_by_id.contains_key(&edge.source) {
            return Err(WorkflowError::Validation(format!(
                "edge {} references missing source {}",
                edge.id, edge.source
            )));
        }
        if !node_by_id.contains_key(&edge.target) {
            return Err(WorkflowError::Validation(format!(
                "edge {} references missing target {}",
                edge.id, edge.target
            )));
        }
        outgoing
            .get_mut(&edge.source)
            .expect("source was checked")
            .push(edge.target.clone());
        incoming
            .get_mut(&edge.target)
            .expect("target was checked")
            .push(edge.source.clone());
    }

    for node in nodes.iter().filter(|node| node.kind == NodeKind::Router) {
        let handles = edges
            .iter()
            .filter(|edge| edge.source == node.id)
            .map(|edge| {
                edge.source_handle.as_deref().ok_or_else(|| {
                    WorkflowError::Validation(format!(
                        "router edge {} requires sourceHandle",
                        edge.id
                    ))
                })
            })
            .collect::<WorkflowResult<Vec<_>>>()?;
        if handles.iter().copied().collect::<BTreeSet<_>>().len() != handles.len() {
            return Err(WorkflowError::Validation(format!(
                "router node {} has duplicate sourceHandle values",
                node.id
            )));
        }
    }

    let start_id = &start_nodes[0];
    let output_id = &output_nodes[0];
    if !incoming[start_id].is_empty() {
        return Err(WorkflowError::Validation(
            "the start node cannot have incoming edges".to_string(),
        ));
    }
    if !outgoing[output_id].is_empty() {
        return Err(WorkflowError::Validation(
            "the output node cannot have outgoing edges".to_string(),
        ));
    }
    for node in nodes {
        if node.id != *start_id && incoming[&node.id].is_empty() {
            return Err(WorkflowError::Validation(format!(
                "node {} is not connected to an upstream node",
                node.id
            )));
        }
        if node.id != *output_id && outgoing[&node.id].is_empty() {
            return Err(WorkflowError::Validation(format!(
                "node {} does not lead to the output node",
                node.id
            )));
        }
    }

    let reachable_from_start = walk(start_id, &outgoing);
    if reachable_from_start.len() != nodes.len() {
        return Err(WorkflowError::Validation(
            "every node must be reachable from the start node".to_string(),
        ));
    }
    let reaches_output = walk(output_id, &incoming);
    if reaches_output.len() != nodes.len() {
        return Err(WorkflowError::Validation(
            "every node must lead to the output node".to_string(),
        ));
    }

    let mut indegree: BTreeMap<String, usize> = incoming
        .iter()
        .map(|(id, sources)| (id.clone(), sources.len()))
        .collect();
    let mut ready: BTreeSet<String> = indegree
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(id.clone()))
        .collect();
    let mut order = Vec::with_capacity(nodes.len());

    while let Some(id) = ready.pop_first() {
        order.push(id.clone());
        for target in &outgoing[&id] {
            let count = indegree
                .get_mut(target)
                .expect("target was validated before sorting");
            *count -= 1;
            if *count == 0 {
                ready.insert(target.clone());
            }
        }
    }

    if order.len() != nodes.len() {
        return Err(WorkflowError::Validation(
            "workflow graphs must be acyclic".to_string(),
        ));
    }
    Ok(order)
}

fn validate_identifier(kind: &str, id: &str) -> WorkflowResult<()> {
    let valid = !id.is_empty()
        && id.len() <= 96
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if !valid {
        return Err(WorkflowError::Validation(format!(
            "{kind} id {id:?} must use 1-96 ASCII letters, numbers, hyphens, or underscores"
        )));
    }
    Ok(())
}

fn walk(start: &str, adjacency: &BTreeMap<String, Vec<String>>) -> BTreeSet<String> {
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::from([start.to_string()]);
    while let Some(id) = queue.pop_front() {
        if !visited.insert(id.clone()) {
            continue;
        }
        if let Some(next) = adjacency.get(&id) {
            queue.extend(next.iter().cloned());
        }
    }
    visited
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::modules::workflow::domain::{NodeData, Position};

    fn node(id: &str, kind: NodeKind) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            kind,
            position: Position { x: 0.0, y: 0.0 },
            data: NodeData {
                label: id.to_string(),
                config: json!({}),
                runtime: Default::default(),
            },
        }
    }

    fn edge(id: &str, source: &str, target: &str) -> WorkflowEdge {
        WorkflowEdge {
            id: id.to_string(),
            source: source.to_string(),
            target: target.to_string(),
            source_handle: None,
        }
    }

    fn valid_graph() -> (Vec<WorkflowNode>, Vec<WorkflowEdge>) {
        (
            vec![
                node("start", NodeKind::Start),
                node("render", NodeKind::Template),
                node("output", NodeKind::Output),
            ],
            vec![edge("a", "start", "render"), edge("b", "render", "output")],
        )
    }

    fn assert_invalid(nodes: &[WorkflowNode], edges: &[WorkflowEdge], message: &str) {
        let error = topological_order(nodes, edges).expect_err("graph must fail");
        assert!(
            error.to_string().contains(message),
            "expected {error} to contain {message:?}"
        );
    }

    #[test]
    fn orders_a_valid_graph() {
        let (nodes, edges) = valid_graph();

        assert_eq!(
            topological_order(&nodes, &edges).expect("valid graph"),
            ["start", "render", "output"]
        );
    }

    #[test]
    fn rejects_cycles() {
        let nodes = vec![
            node("start", NodeKind::Start),
            node("first", NodeKind::Template),
            node("second", NodeKind::Template),
            node("output", NodeKind::Output),
        ];
        let edges = vec![
            WorkflowEdge {
                id: "a".into(),
                source: "start".into(),
                target: "first".into(),
                source_handle: None,
            },
            WorkflowEdge {
                id: "b".into(),
                source: "first".into(),
                target: "second".into(),
                source_handle: None,
            },
            WorkflowEdge {
                id: "c".into(),
                source: "second".into(),
                target: "first".into(),
                source_handle: None,
            },
            WorkflowEdge {
                id: "d".into(),
                source: "second".into(),
                target: "output".into(),
                source_handle: None,
            },
        ];

        let error = topological_order(&nodes, &edges).expect_err("cycle must fail");
        assert!(error.to_string().contains("acyclic"));
    }

    #[test]
    fn rejects_empty_graph_and_wrong_boundary_counts() {
        assert_invalid(&[], &[], "must contain nodes");

        assert_invalid(
            &[
                node("render", NodeKind::Template),
                node("output", NodeKind::Output),
            ],
            &[],
            "exactly one start node, found 0",
        );
        assert_invalid(
            &[
                node("start-a", NodeKind::Start),
                node("start-b", NodeKind::Start),
                node("output", NodeKind::Output),
            ],
            &[],
            "exactly one start node, found 2",
        );
        assert_invalid(
            &[
                node("start", NodeKind::Start),
                node("render", NodeKind::Template),
            ],
            &[],
            "exactly one output node, found 0",
        );
    }

    #[test]
    fn rejects_invalid_duplicate_or_unlabelled_nodes() {
        let (mut nodes, edges) = valid_graph();
        nodes[1].id = "contains space".to_string();
        assert_invalid(&nodes, &edges, "must use 1-96 ASCII");

        let (mut nodes, edges) = valid_graph();
        nodes[1].data.label = "  ".to_string();
        assert_invalid(&nodes, &edges, "must have a label");

        let (mut nodes, edges) = valid_graph();
        nodes[1].id = "start".to_string();
        assert_invalid(&nodes, &edges, "duplicate node id start");
    }

    #[test]
    fn rejects_invalid_duplicate_self_or_dangling_edges() {
        let (nodes, mut edges) = valid_graph();
        edges[0].id = "bad edge".to_string();
        assert_invalid(&nodes, &edges, "must use 1-96 ASCII");

        let (nodes, mut edges) = valid_graph();
        edges[1].id = "a".to_string();
        assert_invalid(&nodes, &edges, "duplicate edge id a");

        let (nodes, mut edges) = valid_graph();
        edges[0].target = "start".to_string();
        assert_invalid(&nodes, &edges, "cannot connect a node to itself");

        let (nodes, mut edges) = valid_graph();
        edges[0].source = "missing".to_string();
        assert_invalid(&nodes, &edges, "references missing source missing");

        let (nodes, mut edges) = valid_graph();
        edges[1].target = "missing".to_string();
        assert_invalid(&nodes, &edges, "references missing target missing");
    }

    #[test]
    fn router_edges_require_unique_named_handles() {
        let nodes = vec![
            node("start", NodeKind::Start),
            node("router", NodeKind::Router),
            node("output", NodeKind::Output),
        ];
        let mut edges = vec![edge("a", "start", "router"), edge("b", "router", "output")];
        assert_invalid(&nodes, &edges, "requires sourceHandle");

        edges[1].source_handle = Some("selected".to_string());
        topological_order(&nodes, &edges).expect("named router edge");

        edges.push(WorkflowEdge {
            id: "c".to_string(),
            source: "router".to_string(),
            target: "output".to_string(),
            source_handle: Some("selected".to_string()),
        });
        assert_invalid(&nodes, &edges, "duplicate sourceHandle values");
    }

    #[test]
    fn enforces_start_and_output_edge_boundaries() {
        let nodes = vec![
            node("start", NodeKind::Start),
            node("output", NodeKind::Output),
        ];
        assert_invalid(
            &nodes,
            &[edge("a", "start", "output"), edge("b", "output", "start")],
            "start node cannot have incoming edges",
        );

        let nodes = vec![
            node("start", NodeKind::Start),
            node("render", NodeKind::Template),
            node("output", NodeKind::Output),
        ];
        assert_invalid(
            &nodes,
            &[
                edge("a", "start", "output"),
                edge("b", "output", "render"),
                edge("c", "render", "output"),
            ],
            "output node cannot have outgoing edges",
        );
    }

    #[test]
    fn rejects_nodes_without_an_upstream_or_output_path() {
        let nodes = vec![
            node("start", NodeKind::Start),
            node("orphan", NodeKind::Template),
            node("output", NodeKind::Output),
        ];
        assert_invalid(
            &nodes,
            &[edge("a", "start", "output"), edge("b", "orphan", "output")],
            "orphan is not connected to an upstream node",
        );
        assert_invalid(
            &nodes,
            &[edge("a", "start", "orphan"), edge("b", "start", "output")],
            "orphan does not lead to the output node",
        );
    }

    #[test]
    fn rejects_disconnected_components_and_branches_that_never_reach_output() {
        let nodes = vec![
            node("start", NodeKind::Start),
            node("first", NodeKind::Template),
            node("second", NodeKind::Template),
            node("output", NodeKind::Output),
        ];
        assert_invalid(
            &nodes,
            &[
                edge("a", "start", "output"),
                edge("b", "first", "second"),
                edge("c", "second", "first"),
            ],
            "every node must be reachable from the start node",
        );
        assert_invalid(
            &nodes,
            &[
                edge("a", "start", "output"),
                edge("b", "start", "first"),
                edge("c", "first", "second"),
                edge("d", "second", "first"),
            ],
            "every node must lead to the output node",
        );
    }
}
