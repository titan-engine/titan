# Opt-in ECS executor design

Issue #6 selects a bounded native executor slice. Sequential execution remains
the default. `ExecutorPolicy::Parallel { max_threads: NonZeroUsize }` opts an
application into bounded concurrency; one thread and WebAssembly use sequential
execution. The limit counts all simultaneously executing system callbacks.

The executor forms contiguous batches in registration order. A candidate joins
only when its declared component/resource accesses are compatible with every
member: shared reads may overlap, writes must be unique, and component/resource
namespaces remain distinct. It does not skip a conflict to find later work.
Exclusive world access, Commands, and ApplyDeferred are full barriers. This
conservative Commands policy preserves entity reservation and command insertion
order without a new allocator or per-thread command queues. Deferred application
remains at explicit boundaries and schedule end, including checked failures.

Typed runners receive prepared SystemContext values. Before starting workers,
the coordinator borrows each world storage/resource once using safe mutable map
iteration. Unique writers receive that mutable reference; readers receive shared
reborrows. Contexts contain only declared borrows. Scoped workers own disjoint
mutable system runners and their contexts, and all join before the world is
accessed again. Components/resources already require Send + Sync and runners
require Send. No raw pointer access, unsafe impl, storage locks, or ECS storage
rewrite is needed. Sealed parameter implementations remain authoritative for
access declarations.

Required resources are checked in registration order while planning a batch.
When a required resource is missing, the valid preceding prefix runs before the
failing system reports its existing error; the failing callback and all later
callbacks remain uncalled. Typed callbacks cannot return additional checked
errors after preparation. Panic is not transactional: all started workers join
before propagation, and their changes may remain. Later batches never start.

Concurrency preserves deterministic ECS results for systems whose effects are
fully described by access metadata. Captured shared state, interior-mutability
side effects, I/O, wall time, and other external effects are outside that promise;
applications needing their registration-order effects should use sequential
execution or an exclusive barrier. No worker pool, work stealing, intra-query
parallelism, automatic default change, or throughput guarantee is included.

Verification must demonstrate actual overlapping callbacks with a bounded
synchronization handshake, read/read sharing, write conflicts across components
and resources, namespace separation, barriers, missing-resource prefix behavior,
deferred failures/order and panic joins. Swarm keeps its closed-form oracle and
compares both policies against the existing baseline, including small-workload
thread overhead. Native and actual-WASM game regressions preserve reference
checksums. Independent design review precedes implementation; final authored
code receives a separate independent review.
