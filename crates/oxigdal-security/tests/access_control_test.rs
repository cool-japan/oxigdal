//! Integration tests for access control.

use oxigdal_security::access_control::{
    AccessContext, AccessControlEvaluator, AccessDecision, AccessRequest, Action, Resource,
    ResourceType, Subject, SubjectType, abac::AbacEngine, permissions::Permission,
    policies::PolicyEngine, rbac::RbacEngine, roles::Role,
};
use std::sync::Arc;

#[test]
fn test_rbac_access_control() {
    let engine = RbacEngine::new();

    let permission = Permission::new(
        "read-dataset".to_string(),
        "Read Dataset".to_string(),
        Action::Read,
        ResourceType::Dataset,
    );

    let mut role = Role::new("viewer".to_string(), "Viewer".to_string());
    role.add_permission("read-dataset".to_string());

    engine
        .add_permission(permission)
        .expect("Failed to add permission");
    engine.add_role(role).expect("Failed to add role");
    engine
        .assign_role("user-123", "viewer")
        .expect("Failed to assign role");

    assert!(engine.has_permission("user-123", Action::Read, ResourceType::Dataset));
    assert!(!engine.has_permission("user-123", Action::Write, ResourceType::Dataset));
}

#[test]
fn test_policy_engine_enforcement() {
    let rbac = Arc::new(RbacEngine::new());
    let abac = Arc::new(AbacEngine::new());
    let engine = PolicyEngine::new(rbac.clone(), abac.clone());

    let permission = Permission::new(
        "read-dataset".to_string(),
        "Read Dataset".to_string(),
        Action::Read,
        ResourceType::Dataset,
    );

    let mut role = Role::new("viewer".to_string(), "Viewer".to_string());
    role.add_permission("read-dataset".to_string());

    rbac.add_permission(permission)
        .expect("Failed to add permission");
    rbac.add_role(role).expect("Failed to add role");
    rbac.assign_role("user-123", "viewer")
        .expect("Failed to assign role");

    let subject = Subject::new("user-123".to_string(), SubjectType::User);
    let resource = Resource::new("dataset-456".to_string(), ResourceType::Dataset);
    let context = AccessContext::new();
    let request = AccessRequest::new(subject, resource, Action::Read, context);

    let decision = engine.evaluate(&request).expect("Evaluation failed");
    assert_eq!(decision, AccessDecision::Allow);
}
