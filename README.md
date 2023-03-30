# rkyv-with

Third-party derive macro for [rkyv](https://github.com/rkyv/rkyv)'s `{Archive/Serialize/Deserialize}With` traits.

The main use-case for this derive is to be able to use `rkyv` on remote types that don't implement `Archive`, `Serialize`, and `Deserialize` themselves.
This provides a somewhat similar workaround to rusts orphan rule as [serde's remote support](https://serde.rs/remote-derive.html).

## Macros

The `ArchiveWith` derive macro implements **both** the `ArchiveWith` and the `SerializeWith` traits. For the `DeserializeWith` trait, use the `DeserializeWith` derive macro.

The `#[archive_with(...)]` attribute helps to fine-tune the implementations.

- `archive_with(from(TypeName))` indicates what the original type is. This attribute is required to be specified at the top level of the type definition. Multiple comma-separated types are allowed, i.e. `from(Type1, Type2)`. The attribute can also be used on fields.
- `archive_with(via(TypeWrapper))` provides a way to convert the type of a field into something else e.g. the unarchivable type contains a `PathBuf` field and in the archivable counterpart it can be a `String` by specifying `via(AsString)`
- `archive_with(getter = "method_name")` must be used in case the unarchivable type includes private fields. The only argument to the specified method must be a reference to the unarchivable type.
- `archive_with(getter_owned)` can be used in case the getter takes an owned instance instead of a reference e.g. if `Copy` is implemented and the getter takes ownership of self.

## Applying the macros

```rust
// Both trait and macro must be imported
use rkyv::with::ArchiveWith;
use rkyv_with::ArchiveWith;

// This could come from some dependency or otherwise remote module.
// Importantly, this does not implement the rkyv traits
struct UnarchivableInner {
    a: u32,
    b: Vec<u8>,
    unimportant: String,
}

#[derive(rkyv::Archive, ArchiveWith)]
#[archive_with(from(UnarchivableInner))] // must be specified
struct ArchivableInner {
    // fields must have the same name
    a: u32,
    #[with(rkyv::with::CopyOptimize)] // archive wrappers work as usual
    b: Vec<u8>,
    // not all fields must be included but if fields
    // are omitted, `DeserializeWith` can not be derived
}

struct UnarchivableOuter {
    archivable_field: i32,
    inner: UnarchivableInner,
    opt: Option<UnarchivableInner>,
    buf: std::path::PathBuf,
}

#[derive(rkyv::Archive, ArchiveWith)]
#[archive_with(from(UnarchivableOuter))]
struct ArchivableOuter {
    // if the field's type is archivable, no annotation is required
    archivable_field: i32,
    // otherwise one must specify the original type through `archive_with(from(...))`
    #[archive_with(from(UnarchivableInner))]
    inner: ArchivableInner,
    // and if the unarchivable type is wrapped with a Vec or Option,
    // it's necessary to specify both the original type, as well as the
    // archivable type wrapped in rkyv's Map wrapper
    #[archive_with(
        from(Option<UnarchivableInner>),
        via(rkyv::with::Map<ArchivableInner>),
    )]
    opt: Option<ArchivableInner>,
    // if the field has a different type, a wrapper that converts
    // the original into the new type must be specified
    #[archive_with(from(std::path::PathBuf), via(rkyv::with::AsString))]
    buf: String,
}
```

## Using the resulting implementations

```rust
use rkyv::with::{ArchiveWith, DeserializeWith, With};
use rkyv::{Archive, Deserialize, Infallible, Serialize};
use rkyv_with::{ArchiveWith, DeserializeWith};

struct Unarchivable {
    a: String,
}

// Can be serialized as usual and
// also serves as serialization wrapper
#[derive(Archive, ArchiveWith, Deserialize, DeserializeWith)]
#[archive_with(from(Unarchivable))]
struct ArchivesTheUnarchivable {
    a: String,
}

// Can be serialized as usual
#[derive(Archive, Serialize)]
struct Container {
    a: i32,
    #[with(ArchivesTheUnarchivable)]
    b: Unarchivable,
}

let unarchivable = Unarchivable { a: String::new() };

// Serialize the instance
let wrapper = With::<_, ArchivesTheUnarchivable>::cast(&unarchivable);
let bytes = rkyv::to_bytes::<_, 32>(wrapper).unwrap();

let archived = unsafe { rkyv::archived_root::<ArchivesTheUnarchivable>(&bytes) };

// Can go back to the original type
let deserialized_unarchivable: Unarchivable =
    ArchivesTheUnarchivable::deserialize_with(archived, &mut Infallible).unwrap();

// Or stick to the wrapper
let deserialized_wrapper: ArchivesTheUnarchivable =
    archived.deserialize(&mut Infallible).unwrap();
```