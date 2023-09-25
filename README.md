# rkyv-with

Third-party derive macro for [rkyv](https://github.com/rkyv/rkyv)'s `{Archive/Serialize/Deserialize}With` traits.

The main use-case for this derive is to be able to use `rkyv` on remote types that don't implement `Archive`, `Serialize`, and `Deserialize` themselves.
This provides a somewhat similar workaround for rusts orphan rule as [serde's remote support](https://serde.rs/remote-derive.html).

## Macros

The `ArchiveWith` derive macro implements **both** the `ArchiveWith` and the `SerializeWith` traits. For the `DeserializeWith` trait, use the `DeserializeWith` derive macro.

The `#[archive_with(...)]` attribute helps to fine-tune the implementations.

- `archive_with(from(TypeName))` indicates what the original type is. This attribute is required to be specified at the top level of the type definition. Multiple comma-separated types are allowed, i.e. `from(Type1, Type2)`. The attribute can also be used on fields.
- `archive_with(via(TypeWrapper))` provides a way to convert the type of a field into something else e.g. the unarchivable type contains a `PathBuf` field and in the archivable counterpart it's a `String` by specifying `via(rkyv::with::AsString)`
- `archive_with(getter = "path::to::function")` must be used in case the unarchivable type includes private fields. The function must be `Fn(&U) -> T` or `Fn(&U) -> &T` where `U` is the unarchivable type and `T` is the field's type.
- `archive_with(getter_owned)` can be specified in addition to `getter = "..."` when the function takes an owned instance instead of a reference.

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
    buf: std::path::PathBuf,
    inner: UnarchivableInner,
    opt: Option<UnarchivableInner>,
    vec: Vec<UnarchivableInner>,
}

#[derive(rkyv::Archive, ArchiveWith)]
#[archive_with(from(UnarchivableOuter))]
struct ArchivableOuter {
    // If the field's type is archivable, no annotation is required
    archivable_field: i32,
    // Otherwise one must specify the original type through `from(...)`
    // as well as a type to convert between the types through `via(...)`
    #[archive_with(from(std::path::PathBuf), via(rkyv::with::AsString))]
    buf: String,
    // If the archivable type itself serves as converter, `via(...)` can be omitted
    #[archive_with(from(UnarchivableInner))]
    inner: ArchivableInner,
    // As such, e.g. for `Vec` and `Option` it's necessary to specify the full
    // original type, as well as the archivable type wrapped in rkyv's Map wrapper
    #[archive_with(
        from(Option<UnarchivableInner>),
        via(rkyv::with::Map<ArchivableInner>),
    )]
    opt: Option<ArchivableInner>,
    // Using the unarchivable type in combination with a with-wrapper is
    // of course also always doable
    #[with(rkyv::with::Map<ArchivableInner>)]
    vec: Vec<UnarchivableInner>,
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

// Or stick with the wrapper
let deserialized_wrapper: ArchivesTheUnarchivable =
    archived.deserialize(&mut Infallible).unwrap();
```

## Private fields

If fields are not directly accessible due to them being private, deriving the traits requires manual specification of getter functions.

```rust
use rkyv::with::ArchiveWith;
use rkyv::Archive;
use rkyv_with::ArchiveWith;

// Imagine again that this is a remote module that you have no way of modifying
mod remote {
    #[derive(Clone)]
    pub struct Remote {
        pub public_field: u32,
        private_field: u32,
    }

    impl Remote {
        // By default it will be assumed that the function will take a reference
        pub fn to_private_field(&self) -> u32 {
            self.private_field
        }

        // If it takes ownership instead, `getter_owned` will need to be added as seen below
        pub fn into_private_field(self) -> u32 {
            self.private_field
        }

        // A publicly available way of creating the type in case you need to deserialize
        pub fn new(public_field: u32, private_field: u32) -> Self {
            Self { public_field, private_field }
        }
    }
}

#[derive(Archive, ArchiveWith)]
#[archive_with(from(remote::Remote))]
struct NativeByReference {
    public_field: u32,
    // Specifying the path to the getter function that takes a reference
    #[archive_with(getter = "remote::Remote::to_private_field")]
    private_field: u32,
}

#[derive(Archive, ArchiveWith)]
#[archive_with(from(remote::Remote))]
struct NativeByValue {
    public_field: u32,
    // If the function takes ownership, be sure to also specify `getter_owned`
    #[archive_with(getter = "remote::Remote::into_private_field", getter_owned)]
    private_field: u32,
}

// Since creating instances of a type that has private fields cannot be done in a general way,
// `DeserializeWith` cannot be derived for such types and instead has to be implemented manually.
// An implementation for the example above could look as follows:

use rkyv::with::DeserializeWith;
use rkyv::{Archived, Fallible};

impl<D: Fallible> DeserializeWith<Archived<NativeByValue>, remote::Remote, D> for NativeByValue {
    fn deserialize_with(
        archived: &Archived<NativeByValue>,
        _deserializer: &mut D,
    ) -> Result<remote::Remote, <D as Fallible>::Error> {
        // Use whichever method is available to create an instance
        Ok(remote::Remote::new(archived.public_field, archived.private_field))
    }
}
```