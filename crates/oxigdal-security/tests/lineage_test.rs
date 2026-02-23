//! Integration tests for lineage tracking.

use oxigdal_security::lineage::{
    EdgeType, LineageEdge, LineageEvent, LineageNode, NodeType, graph::LineageGraph,
};

#[test]
fn test_lineage_tracking() {
    let graph = LineageGraph::new();

    let input_node = LineageNode::new(NodeType::Dataset, "input-dataset".to_string());
    let input_id = graph.add_node(input_node).expect("Failed to add node");

    let output_node = LineageNode::new(NodeType::Dataset, "output-dataset".to_string());
    let output_id = graph.add_node(output_node).expect("Failed to add node");

    let edge = LineageEdge::new(input_id.clone(), output_id.clone(), EdgeType::DerivedFrom);
    graph.add_edge(edge).expect("Failed to add edge");

    let upstream = graph
        .get_upstream(&output_id)
        .expect("Failed to get upstream");
    assert_eq!(upstream.len(), 1);
    assert_eq!(upstream[0].id, input_id);
}

#[test]
fn test_lineage_event_recording() {
    let graph = LineageGraph::new();

    let input_node = LineageNode::new(NodeType::Dataset, "input-1".to_string());
    graph.add_node(input_node).expect("Failed to add node");

    let output_node = LineageNode::new(NodeType::Dataset, "output-1".to_string());
    graph.add_node(output_node).expect("Failed to add node");

    let event = LineageEvent::new("transform".to_string())
        .with_input("input-1".to_string())
        .with_output("output-1".to_string())
        .with_operation("reproject".to_string());

    graph.record_event(event).expect("Failed to record event");

    let (nodes, edges) = graph.stats();
    assert!(nodes >= 2);
    assert!(edges >= 2);
}
