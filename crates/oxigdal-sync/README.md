# oxigdal-sync

Multi-device synchronization library for OxiGDAL with CRDTs, vector clocks, and operational transformation.

## Features

- **CRDTs (Conflict-free Replicated Data Types)**
  - LWW-Register (Last-Write-Wins Register)
  - G-Counter (Grow-only Counter)
  - PN-Counter (Positive-Negative Counter)
  - OR-Set (Observed-Remove Set)

- **Vector Clocks** for causality tracking in distributed systems

- **Operational Transformation** for concurrent text editing
  - Text operations (insert, delete, retain)
  - Operation composition and transformation
  - Conflict resolution

- **Multi-device Coordination**
  - Device registration and status management
  - Sync session tracking
  - State synchronization protocols

- **Merkle Trees** for efficient change detection and verification

- **Delta Encoding** for bandwidth-efficient data transfer

## Usage

### CRDTs

```rust
use oxigdal_sync::crdt::{LwwRegister, GCounter, PnCounter, OrSet};

// Last-Write-Wins Register
let mut reg = LwwRegister::new("device-1".to_string(), "initial".to_string());
reg.set("updated".to_string());

// G-Counter (grow-only)
let mut counter = GCounter::new("device-1".to_string());
counter.increment(5);

// PN-Counter (increment/decrement)
let mut counter = PnCounter::new("device-1".to_string());
counter.increment(10);
counter.decrement(3);

// OR-Set
let mut set = OrSet::new("device-1".to_string());
set.insert("apple".to_string());
set.insert("banana".to_string());
```

### Vector Clocks

```rust
use oxigdal_sync::vector_clock::{VectorClock, ClockOrdering};

let mut clock1 = VectorClock::new("device-1".to_string());
let mut clock2 = VectorClock::new("device-2".to_string());

clock1.tick();
clock2.tick();

match clock1.compare(&clock2) {
    ClockOrdering::Before => println!("clock1 happened before clock2"),
    ClockOrdering::After => println!("clock1 happened after clock2"),
    ClockOrdering::Concurrent => println!("concurrent events"),
    ClockOrdering::Equal => println!("same event"),
}
```

### Multi-device Coordination

```rust
use oxigdal_sync::coordinator::SyncCoordinator;

let coordinator = SyncCoordinator::new("device-1".to_string());

// Register devices
coordinator.register_device("device-2".to_string())?;

// Start sync session
let session = coordinator.start_sync_session("device-2".to_string())?;

// ... perform synchronization ...

// Complete session
coordinator.complete_sync_session(&session.session_id)?;
```

### Merkle Trees

```rust
use oxigdal_sync::merkle::MerkleTree;

let data = vec![
    b"block1".to_vec(),
    b"block2".to_vec(),
    b"block3".to_vec(),
];

let tree = MerkleTree::from_data(data.clone())?;

// Verify data
assert!(tree.verify(&data)?);

// Compare trees
let differences = tree1.diff(&tree2);
```

### Delta Encoding

```rust
use oxigdal_sync::delta::DeltaEncoder;

let encoder = DeltaEncoder::default_encoder();
let base = b"hello world";
let target = b"hello world!";

let delta = encoder.encode(base, target)?;
let result = delta.apply(base)?;

assert_eq!(result, target);
```

## License

Apache-2.0
