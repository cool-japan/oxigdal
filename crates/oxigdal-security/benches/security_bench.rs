//! Security performance benchmarks.
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_security::{
    access_control::{
        AccessContext, AccessControlEvaluator, AccessRequest, Action, Resource, ResourceType,
        Subject, SubjectType, permissions::Permission, rbac::RbacEngine, roles::Role,
    },
    encryption::{EncryptionAlgorithm, at_rest::AtRestEncryptor},
};
use std::hint::black_box;

fn encryption_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("encryption");

    let sizes = vec![1024, 10240, 102400]; // 1KB, 10KB, 100KB

    for size in sizes {
        let data = vec![0u8; size];
        let key = AtRestEncryptor::generate_key(EncryptionAlgorithm::Aes256Gcm);
        let encryptor =
            AtRestEncryptor::new(EncryptionAlgorithm::Aes256Gcm, key, "bench-key".to_string())
                .expect("Failed to create encryptor");

        group.bench_with_input(
            BenchmarkId::new("aes_gcm_encrypt", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let _ = encryptor.encrypt(black_box(data), None);
                });
            },
        );

        let encrypted = encryptor.encrypt(&data, None).expect("Encryption failed");
        group.bench_with_input(
            BenchmarkId::new("aes_gcm_decrypt", size),
            &encrypted,
            |b, encrypted| {
                b.iter(|| {
                    let _ = encryptor.decrypt(black_box(encrypted));
                });
            },
        );
    }

    group.finish();
}

fn access_control_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("access_control");

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

    let subject = Subject::new("user-123".to_string(), SubjectType::User);
    let resource = Resource::new("dataset-456".to_string(), ResourceType::Dataset);
    let context = AccessContext::new();
    let request = AccessRequest::new(subject, resource, Action::Read, context);

    group.bench_function("rbac_evaluation", |b| {
        b.iter(|| {
            let _ = engine.evaluate(black_box(&request));
        });
    });

    group.finish();
}

fn key_derivation_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("key_derivation");

    use oxigdal_security::encryption::{KeyDerivationParams, derive_key};

    let password = b"test_password";
    let salt = b"test_salt_12345678";

    let pbkdf2_params = KeyDerivationParams::pbkdf2_recommended(salt.to_vec());
    group.bench_function("pbkdf2", |b| {
        b.iter(|| {
            let _ = derive_key(black_box(password), black_box(&pbkdf2_params), 32);
        });
    });

    let argon2_params = KeyDerivationParams::argon2_recommended(salt.to_vec());
    group.bench_function("argon2", |b| {
        b.iter(|| {
            let _ = derive_key(black_box(password), black_box(&argon2_params), 32);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    encryption_benchmark,
    access_control_benchmark,
    key_derivation_benchmark
);
criterion_main!(benches);
