# SOLID Design Principles --- MCW Launcher (Rust)

> **Purpose:** A practical design standard for reworking the MCW
> Launcher Rust codebase.
>
> **Scope:** Core domain logic, application services, infrastructure,
> persistence, loader integrations, and GUI boundaries.
>
> **Primary rule:** Apply SOLID to reduce coupling and make change
> safer. Do not turn it into a requirement to create an interface,
> trait, wrapper, or module for every type.

------------------------------------------------------------------------

## 1. Design Goals

The Rust codebase should:

1.  Keep domain and application rules independent from the GUI toolkit
    and platform-specific details.
2.  Make responsibilities and ownership boundaries clear.
3.  Allow new Minecraft versions, loaders, launch options, and
    integrations without destabilizing existing behavior.
4.  Make important logic testable without starting the GUI or requiring
    a real Minecraft installation.
5.  Prefer explicit data flow and compile-time guarantees over hidden
    global state or runtime indirection.
6.  Remain understandable to contributors who are new to the project.

SOLID is a set of design principles, not a rigid architecture. A simple
implementation is preferable when it satisfies the requirements clearly.

## 2. S --- Single Responsibility Principle (SRP)

**Definition:** A module should have one coherent responsibility and one
primary reason to change.

A type may contain multiple methods. SRP is about whether those methods
serve the same responsibility, not about keeping classes or files
artificially small.

### MCW Launcher guidance

Keep these responsibilities separate:

  -----------------------------------------------------------------------
  Responsibility                      Suitable owner
  ----------------------------------- -----------------------------------
  Instance lifecycle rules            `InstanceService` or a focused
                                      instance manager

  Instance persistence                `InstanceRepository` implementation

  Minecraft metadata retrieval        Metadata client / provider

  Loader installation workflow        Loader-specific installer or
                                      installation service

  Java discovery                      `JavaRuntimeDetector`

  Process construction and execution  Launch/process subsystem

  GUI presentation and interaction    GUI layer

  Configuration serialization         Configuration storage adapter
  -----------------------------------------------------------------------

Avoid a single type that manages instance rules, downloads files, writes
configuration, starts processes, and updates GUI widgets.

### Rust example

``` rust
pub struct InstanceService<R> {
    repository: R,
}

impl<R: InstanceRepository> InstanceService<R> {
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    pub fn rename_instance(
        &self,
        id: InstanceId,
        new_name: InstanceName,
    ) -> Result<(), InstanceError> {
        let mut instance = self.repository.load(&id)?;
        instance.rename(new_name)?;
        self.repository.save(&instance)
    }
}
```

The service coordinates an instance use case. It does not need to know
how the repository stores data or how a screen displays the result.

### Warning signs

-   A type changes whenever unrelated subsystems change.
-   A manager contains HTTP, filesystem, process, and GUI code.
-   A model directly downloads data or modifies widgets.
-   A GUI component contains business rules that are also needed by
    tests or other interfaces.

### Review question

> If one responsibility changes, can that change be localized to a
> small, predictable part of the codebase?

------------------------------------------------------------------------

## 3. O --- Open/Closed Principle (OCP)

**Definition:** Software entities should be open for extension but
closed for modification.

Prefer designs where a new supported behavior can be added through a
well-defined extension point instead of repeatedly editing a central
conditional or destabilizing established behavior.

### MCW Launcher guidance

Potential extension points include:

-   Minecraft loader integrations (Vanilla, Fabric, Forge, NeoForge,
    Quilt).
-   Authentication providers.
-   Metadata sources.
-   Launch argument policies.
-   Modpack import sources.
-   Platform-specific process configuration.

Do not introduce a plugin framework merely because OCP exists. Use an
abstraction when there are multiple implementations, a credible
near-term extension, or a clear testing boundary.

### Rust example: loader installation

``` rust
pub trait LoaderInstaller {
    fn loader_id(&self) -> LoaderId;

    fn install(
        &self,
        request: &InstallRequest,
    ) -> Result<InstallPlan, InstallError>;
}

pub struct FabricInstaller {
    // Dependencies required by Fabric installation.
}

impl LoaderInstaller for FabricInstaller {
    fn loader_id(&self) -> LoaderId {
        LoaderId::Fabric
    }

    fn install(
        &self,
        request: &InstallRequest,
    ) -> Result<InstallPlan, InstallError> {
        // Build a Fabric installation plan.
        todo!()
    }
}
```

A registry or application service can select the implementation. Keep
loader-specific behavior inside the loader integration rather than
spreading loader checks across the GUI and core.

### Rust-specific considerations

-   Use traits when behavior is genuinely substitutable.
-   Use enums when the set of variants is intentionally closed and
    known.
-   Prefer exhaustive `match` statements when adding a new variant
    should force review of all affected logic.
-   Avoid string-based type dispatch when a typed enum or identifier is
    appropriate.
-   Avoid trait objects (`Box<dyn Trait>`, `Arc<dyn Trait>`) unless
    runtime polymorphism is actually useful. Generics and enums are
    often simpler.

### Warning signs

-   The same loader-specific `match` is duplicated across multiple
    modules.
-   Adding one loader requires editing unrelated GUI, persistence, and
    launch code.
-   A generic framework has more complexity than the features it
    supports.

### Review question

> Can a new implementation be added at the intended boundary without
> modifying unrelated components?

------------------------------------------------------------------------

## 4. L --- Liskov Substitution Principle (LSP)

**Definition:** A subtype or implementation must be usable wherever its
abstraction is expected without breaking the abstraction's documented
behavior.

In Rust, this applies to trait implementations and enum variants just as
it does to classical object-oriented inheritance.

### MCW Launcher guidance

If several implementations satisfy a trait, they must honor the same
contract:

-   The same meaning of success and failure.
-   Consistent validation guarantees.
-   No unexpected weakening of required behavior.
-   No hidden assumptions that only one implementation satisfies.
-   Clear handling of unsupported capabilities.

For example, every `LoaderInstaller` implementation should interpret the
installation request consistently and return a valid installation plan
or a documented error.

### Rust example: define a meaningful contract

``` rust
pub trait InstanceRepository {
    /// Returns `Ok(None)` when the instance does not exist.
    /// Returns `Err` when the repository cannot complete the read.
    fn find(
        &self,
        id: &InstanceId,
    ) -> Result<Option<Instance>, RepositoryError>;

    /// Persists the supplied instance or returns an error.
    fn save(
        &self,
        instance: &Instance,
    ) -> Result<(), RepositoryError>;
}
```

An in-memory repository and a filesystem repository must preserve these
semantics. One implementation must not return an error for a missing
record if the contract says missing records are represented by
`Ok(None)`.

### Do not force false substitutability

Not every loader supports every feature. Do not create a broad trait
that promises capabilities some implementations cannot provide.

Prefer:

-   Smaller capability traits.
-   Explicit capability metadata.
-   A typed `UnsupportedCapability` result where appropriate.
-   Separate workflows when behavior is fundamentally different.

### Warning signs

-   An implementation panics for a normal, documented input.
-   A method silently does nothing despite promising an operation.
-   One implementation changes the meaning of a return value.
-   Callers must downcast or inspect concrete types to make the
    abstraction work.

### Review question

> Can every implementation honor the same contract, including its error
> and edge-case behavior?

------------------------------------------------------------------------

## 5. I --- Interface Segregation Principle (ISP)

**Definition:** Clients should not be forced to depend on methods or
capabilities they do not use.

In Rust, keep traits focused around cohesive capabilities. A trait
should express a useful contract, not collect every operation in a
subsystem.

### MCW Launcher guidance

Avoid one enormous trait such as:

``` rust
pub trait MinecraftBackend {
    fn install(&self);
    fn update(&self);
    fn uninstall(&self);
    fn authenticate(&self);
    fn download_assets(&self);
    fn launch(&self);
    fn export_modpack(&self);
}
```

An implementation may not support all of these capabilities.

Prefer smaller traits where there is a real need for independent use:

``` rust
pub trait InstanceReader {
    fn load(&self, id: &InstanceId)
        -> Result<Option<Instance>, RepositoryError>;
}

pub trait InstanceWriter {
    fn save(&self, instance: &Instance)
        -> Result<(), RepositoryError>;
}

pub trait ProcessRunner {
    fn run(
        &self,
        command: &LaunchCommand,
    ) -> Result<ProcessHandle, ProcessError>;
}
```

A component should depend only on the capabilities it needs.

### Important nuance

Do not split every trait into a single method by default. A cohesive
trait with several related methods is often clearer and easier to use
than many tiny traits.

### Warning signs

-   Implementations contain placeholder methods solely to satisfy a
    large trait.
-   Callers depend on a broad interface but use only one small part.
-   A change to an unrelated trait method breaks many implementations.

### Review question

> Does each consumer depend only on the capabilities it actually needs?

------------------------------------------------------------------------

## 6. D --- Dependency Inversion Principle (DIP)

**Definition:** High-level policy should not depend directly on
low-level implementation details. Both should depend on abstractions
owned by the appropriate boundary.

This principle is central to keeping MCW Launcher's domain and
application logic independent from GUI, storage, network, and
operating-system details.

### MCW Launcher guidance

The application layer may define the capabilities it needs.
Infrastructure provides implementations.

Examples:

  -------------------------------------------------------------------------
  High-level policy needs Boundary abstraction      Infrastructure
                                                    implementation
  ----------------------- ------------------------- -----------------------
  Load and save instances `InstanceRepository`      JSON / filesystem
                                                    repository

  Fetch version metadata  `VersionMetadataSource`   HTTP client

  Execute a launch        `ProcessRunner`           OS process
  command                                           implementation

  Find installed Java     `JavaRuntimeProvider`     Platform detector
  runtimes                                          

  Read application        `SettingsRepository`      Configuration file
  settings                                          adapter
  -------------------------------------------------------------------------

### Rust example: dependency injection

``` rust
pub trait InstanceRepository {
    fn save(
        &self,
        instance: &Instance,
    ) -> Result<(), RepositoryError>;
}

pub struct InstanceService<R> {
    repository: R,
}

impl<R: InstanceRepository> InstanceService<R> {
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    pub fn save_instance(
        &self,
        instance: &Instance,
    ) -> Result<(), RepositoryError> {
        self.repository.save(instance)
    }
}
```

The composition root creates the concrete dependencies:

``` rust
let repository = JsonInstanceRepository::new(instance_directory);
let instance_service = InstanceService::new(repository);
```

The service does not construct its own filesystem repository. Dependency
construction belongs at the application boundary.

### Rust-specific considerations

-   Prefer generics for static dispatch when the implementation is known
    at compile time.
-   Use `dyn Trait` when runtime selection or heterogeneous collections
    justify it.
-   Use `Arc<T>` when shared ownership across threads or components is
    genuinely needed.
-   Avoid global mutable state and service locators as substitutes for
    explicit dependencies.
-   Keep platform-specific APIs behind infrastructure boundaries.
-   Keep GUI framework types out of domain models and application
    contracts.

### Warning signs

-   Core logic imports GUI toolkit modules.
-   Domain models perform HTTP requests or filesystem operations.
-   High-level services instantiate concrete infrastructure types
    internally.
-   Tests require real network access or a full GUI just to exercise
    business rules.

### Review question

> Can the high-level policy be tested with a small fake or in-memory
> implementation without using real infrastructure?

------------------------------------------------------------------------

## 7. Applying SOLID Across the MCW Launcher Architecture

SOLID works best alongside explicit architectural boundaries.

A practical dependency direction is:

``` text
GUI / Presentation
        |
        v
Application / Use Cases
        |
        v
Domain

Infrastructure implements ports
defined by the appropriate inner layer.
```

The diagram describes dependency direction, not necessarily a strict
directory structure. Infrastructure may be called by application
workflows through injected interfaces, but domain policy must not depend
on infrastructure details.

### Suggested responsibility boundaries

  -----------------------------------------------------------------------
  Area                                Responsibility
  ----------------------------------- -----------------------------------
  `domain`                            Entities, value objects,
                                      invariants, domain errors

  `application`                       Use cases, orchestration, ports,
                                      application errors

  `infrastructure`                    Filesystem, HTTP, process
                                      execution, OS integration

  `presentation`                      GUI state, widgets, user
                                      interaction, view formatting

  `bootstrap` / composition root      Construct implementations and wire
                                      dependencies
  -----------------------------------------------------------------------

Adapt these names to the existing repository. Do not reorganize the
entire project solely to match this example.

### Example dependency rules

**Domain** - Must not import GUI, HTTP, filesystem, or platform-specific
crates. - Should contain rules that are meaningful independently of
presentation. - Should not know where its data is stored.

**Application** - Coordinates domain behavior and defines the ports it
needs. - Should not depend on concrete filesystem or HTTP clients. -
Should return application-level results rather than GUI widgets or
toolkit-specific objects.

**Infrastructure** - Implements application ports. - Owns serialization,
network requests, process spawning, and platform-specific behavior. -
Translates external failures into errors meaningful to the application
boundary.

**Presentation** - Converts user actions into application requests. -
Displays application state and errors. - Should not duplicate domain
rules or directly manipulate persistence formats.

------------------------------------------------------------------------

## 8. Rust Engineering Rules

SOLID should be expressed idiomatically in Rust.

### Prefer

-   Structs and enums for explicit data modeling.
-   Traits for stable behavioral contracts.
-   Ownership and borrowing over unnecessary shared mutable state.
-   `Result<T, E>` for recoverable failures.
-   Typed identifiers and value objects where they prevent invalid
    states.
-   Exhaustive matching for intentionally closed sets of variants.
-   Small, cohesive modules with clear public APIs.
-   Dependency injection through constructors or explicit application
    state.
-   Test doubles or in-memory adapters at meaningful boundaries.

### Avoid

-   Creating a trait for every struct without a real substitution need.
-   Using `dyn Trait` everywhere by default.
-   Deep abstraction layers that merely forward method calls.
-   Excessive generic parameters that make the code harder to
    understand.
-   Large managers that become service locators.
-   Domain models coupled to GUI or serialization frameworks.
-   Duplicated business rules across GUI and core.
-   Premature plugin systems or speculative extensibility.
-   Splitting modules solely to reduce line counts.

### Errors and contracts

-   Document important trait behavior, including missing-data semantics
    and failure conditions.
-   Do not use `unwrap()` or `expect()` for routine external failures.
-   Preserve useful error context when translating errors between
    layers.
-   Avoid leaking low-level implementation errors into user-facing
    messages.
-   Do not silently swallow errors to make an interface appear
    successful.

------------------------------------------------------------------------

## 9. Codebase Rework Checklist

Use this checklist during review and refactoring. Apply it incrementally
rather than attempting a risky rewrite in one pass.

### Responsibility and boundaries

-   [ ] Each module has a clear, cohesive responsibility.
-   [ ] GUI code does not own domain or persistence rules.
-   [ ] Domain models do not perform external I/O.
-   [ ] Managers do not become catch-all service containers.
-   [ ] Serialization and API response types are not automatically
    treated as domain models.

### Extensibility and contracts

-   [ ] Loader-specific behavior is localized.
-   [ ] Shared traits have clear contracts.
-   [ ] Implementations preserve the semantics of their traits.
-   [ ] Unsupported capabilities are explicit.
-   [ ] New variants do not require unrelated changes.

### Dependencies

-   [ ] High-level application logic depends on abstractions where
    useful.
-   [ ] Infrastructure implementations are wired at the composition
    root.
-   [ ] Core modules do not import GUI or platform-specific details.
-   [ ] Dependencies are explicit and testable.
-   [ ] Runtime polymorphism is used only when needed.

### Maintainability and tests

-   [ ] Important business rules have unit tests.
-   [ ] Repository and process boundaries have isolated tests.
-   [ ] Error paths are tested, not just successful paths.
-   [ ] Public APIs are cohesive and documented.
-   [ ] Refactoring preserves existing behavior unless a behavior change
    is intentional.
-   [ ] Tests do not require network access or a running GUI unless the
    test is specifically an integration test.

------------------------------------------------------------------------

## 10. Refactoring Workflow

For each subsystem:

1.  **Understand:** Identify its current responsibilities, callers,
    dependencies, and observable behavior.
2.  **Classify:** Identify concrete SRP, OCP, LSP, ISP, or DIP problems.
    Do not label code as a violation merely because it looks different
    from an example.
3.  **Define the boundary:** Decide which layer owns the policy and
    which layer owns external details.
4.  **Design the smallest change:** Introduce only the traits, types, or
    modules needed to solve the identified problem.
5.  **Preserve behavior:** Keep existing public behavior and data
    compatibility unless the change explicitly requires otherwise.
6.  **Test:** Add or update unit and integration tests before removing
    the old path.
7.  **Migrate:** Move callers to the new boundary and remove obsolete
    code.
8.  **Review:** Check dependency direction, error behavior, API clarity,
    and unnecessary abstraction.

Do not combine unrelated architectural changes into one large refactor.
Prefer small, reviewable changes that can be tested and reverted
independently.

------------------------------------------------------------------------

## 11. Definition of Done

A subsystem rework is complete when:

-   Its responsibilities and public contracts are clear.
-   Dependencies point in the intended direction.
-   External details are isolated behind appropriate boundaries.
-   Implementations honor their documented contracts.
-   Unsupported behavior and failure cases are explicit.
-   Relevant tests pass.
-   Existing user data and supported workflows remain compatible.
-   The design is no more abstract or complicated than the requirements
    justify.

------------------------------------------------------------------------

## Final Principle

**SOLID is a tool for managing change, not a score to maximize.**

For MCW Launcher, prioritize clear ownership, stable contracts,
dependency direction, data compatibility, and testability. A small
design that is easy to understand and safely extend is better than an
elaborate architecture that exists only to demonstrate SOLID.
