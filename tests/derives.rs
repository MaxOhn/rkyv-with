use std::{fmt::Debug, num::NonZeroU64, path::PathBuf};

use rkyv::{
    ser::Serializer,
    with::{ArchiveWith, AsString, CopyOptimize, DeserializeWith, Map, Niche, SerializeWith, With},
    AlignedVec, Archive, Archived, Infallible,
};
use rkyv_with::{ArchiveWith, DeserializeWith};
use serializer::CustomSerializer;

use crate::with_noop::WithNoop;

mod serializer {
    use std::{alloc::Layout, ptr::NonNull};

    use rkyv::{
        ser::{serializers::AllocSerializer, ScratchSpace, Serializer},
        with::AsStringError,
        AlignedVec, Fallible,
    };

    /// Custom serializer so we can use the `AsString` wrapper
    #[derive(Default)]
    pub struct CustomSerializer<const N: usize> {
        inner: AllocSerializer<N>,
    }

    impl<const N: usize> CustomSerializer<N> {
        pub fn into_bytes(self) -> AlignedVec {
            self.inner.into_serializer().into_inner()
        }
    }

    impl<const N: usize> Fallible for CustomSerializer<N> {
        type Error = CustomSerializerError<<AllocSerializer<N> as Fallible>::Error>;
    }

    impl<const N: usize> Serializer for CustomSerializer<N> {
        #[inline]
        fn pos(&self) -> usize {
            self.inner.pos()
        }

        #[inline]
        fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.inner
                .write(bytes)
                .map_err(CustomSerializerError::Inner)
        }
    }

    impl<const N: usize> ScratchSpace for CustomSerializer<N> {
        unsafe fn push_scratch(&mut self, layout: Layout) -> Result<NonNull<[u8]>, Self::Error> {
            self.inner
                .push_scratch(layout)
                .map_err(CustomSerializerError::Inner)
        }

        unsafe fn pop_scratch(
            &mut self,
            ptr: NonNull<u8>,
            layout: Layout,
        ) -> Result<(), Self::Error> {
            self.inner
                .pop_scratch(ptr, layout)
                .map_err(CustomSerializerError::Inner)
        }
    }

    #[derive(Debug)]
    pub enum CustomSerializerError<E> {
        Inner(E),
        AsStringError(AsStringError),
    }

    impl<E> From<AsStringError> for CustomSerializerError<E> {
        fn from(err: AsStringError) -> Self {
            Self::AsStringError(err)
        }
    }
}

mod with_noop {
    use rkyv::{
        with::{ArchiveWith, DeserializeWith, SerializeWith},
        Archive, Archived, Deserialize, Fallible, Serialize,
    };

    /// Usable as rkyv With-wrapper which doesn't to anything
    /// and just uses the type's Archive/Deserialize/Serialize impls.
    pub struct WithNoop;

    impl<F: Archive> ArchiveWith<F> for WithNoop {
        type Archived = <F as Archive>::Archived;
        type Resolver = <F as Archive>::Resolver;

        unsafe fn resolve_with(
            field: &F,
            pos: usize,
            resolver: Self::Resolver,
            out: *mut Self::Archived,
        ) {
            field.resolve(pos, resolver, out)
        }
    }

    impl<F: Serialize<S>, S: Fallible> SerializeWith<F, S> for WithNoop {
        fn serialize_with(
            field: &F,
            serializer: &mut S,
        ) -> Result<Self::Resolver, <S as Fallible>::Error> {
            field.serialize(serializer)
        }
    }

    impl<F, D> DeserializeWith<Archived<F>, F, D> for WithNoop
    where
        F: Archive,
        Archived<F>: Deserialize<F, D>,
        D: Fallible,
    {
        fn deserialize_with(
            field: &Archived<F>,
            deserializer: &mut D,
        ) -> Result<F, <D as Fallible>::Error> {
            field.deserialize(deserializer)
        }
    }
}

fn serialize<Wrapper, Remote>(remote: &Remote) -> AlignedVec
where
    Wrapper: SerializeWith<Remote, CustomSerializer<8>>,
{
    let mut serializer = CustomSerializer::<8>::default();
    let with = With::<Remote, Wrapper>::cast(remote);
    serializer.serialize_value(with).unwrap();

    serializer.into_bytes()
}

fn archive<Wrapper, Remote>(bytes: &[u8]) -> &<Wrapper as ArchiveWith<Remote>>::Archived
where
    Wrapper: ArchiveWith<Remote>,
{
    unsafe { rkyv::archived_root::<With<Remote, Wrapper>>(bytes) }
}

fn roundtrip<Wrapper, Remote>(remote: &Remote)
where
    Wrapper: ArchiveWith<Remote>
        + SerializeWith<Remote, CustomSerializer<8>>
        + DeserializeWith<Archived<With<Remote, Wrapper>>, Remote, Infallible>,
    Remote: Debug + PartialEq,
{
    let bytes = serialize::<Wrapper, Remote>(remote);
    let archived = archive::<Wrapper, Remote>(&bytes);
    let deserialized: Remote = Wrapper::deserialize_with(archived, &mut Infallible).unwrap();

    assert_eq!(remote, &deserialized);
}

#[test]
fn named_struct() {
    #[derive(Debug, PartialEq)]
    struct Remote<A> {
        a: u8,
        b: Vec<A>,
        c: Option<NonZeroU64>,
        d: Vec<PathBuf>,
    }

    #[derive(Archive, ArchiveWith, DeserializeWith)]
    #[archive_with(from(Remote::<A>))]
    struct Example<A> {
        a: u8,
        #[with(CopyOptimize)]
        b: Vec<A>,
        #[with(Niche)]
        #[archive_with(from(Option<NonZeroU64>), via(Niche))]
        c: Option<NonZeroU64>,
        #[archive_with(from(Vec<PathBuf>), via(WithNoop, Map<AsString>))]
        d: Vec<String>,
    }

    let remote = Remote {
        a: 0,
        b: Vec::new(),
        c: None,
        d: Vec::new(),
    };

    roundtrip::<Example<i32>, _>(&remote);
}

#[test]
fn unnamed_struct() {
    #[derive(Debug, PartialEq)]
    struct Remote<A>(u8, Vec<A>, Option<NonZeroU64>, Vec<PathBuf>);

    #[derive(Archive, ArchiveWith, DeserializeWith)]
    #[archive_with(from(Remote::<A>))]
    struct Example<A>(
        u8,
        #[with(CopyOptimize)] Vec<A>,
        #[with(Niche)]
        #[archive_with(from(Option<NonZeroU64>), via(Niche))]
        Option<NonZeroU64>,
        #[archive_with(from(Vec<PathBuf>), via(WithNoop, Map<AsString>))] Vec<String>,
    );

    let remote = Remote(0, Vec::new(), None, Vec::new());
    roundtrip::<Example<i32>, _>(&remote);
}

#[test]
fn unit_struct() {
    #[derive(Debug, PartialEq)]
    struct Remote;

    #[derive(Archive, ArchiveWith, DeserializeWith)]
    #[archive_with(from(Remote))]
    struct Example;

    let remote = Remote;
    roundtrip::<Example, _>(&remote);
}

#[test]
fn full_enum() {
    #[derive(Debug, PartialEq)]
    enum Remote<A> {
        A,
        B(u8),
        C {
            a: Vec<A>,
            b: Option<NonZeroU64>,
            c: Vec<PathBuf>,
        },
    }

    #[allow(unused)]
    #[derive(Archive, ArchiveWith, DeserializeWith)]
    #[archive_with(from(Remote::<A>))]
    enum Example<A> {
        A,
        B(u8),
        C {
            #[with(CopyOptimize)]
            a: Vec<A>,
            #[with(Niche)]
            #[archive_with(from(Option<NonZeroU64>), via(Niche))]
            b: Option<NonZeroU64>,
            #[archive_with(from(Vec<PathBuf>), via(WithNoop, Map<AsString>))]
            c: Vec<String>,
        },
    }

    for remote in [
        Remote::A,
        Remote::B(0),
        Remote::C {
            a: Vec::new(),
            b: None,
            c: Vec::new(),
        },
    ] {
        roundtrip::<Example<i32>, _>(&remote);
    }
}

#[test]
fn named_struct_private() {
    mod remote {
        #[derive(Copy, Clone, Default)]
        pub struct Remote {
            inner: [u8; 4],
        }

        impl Remote {
            pub fn into_inner(self) -> [u8; 4] {
                self.inner
            }

            pub fn to_inner(&self) -> [u8; 4] {
                self.inner
            }

            pub fn as_inner(&self) -> &[u8; 4] {
                &self.inner
            }
        }
    }

    #[derive(Archive, ArchiveWith)]
    #[archive_with(from(remote::Remote))]
    struct ExampleByVal {
        #[archive_with(getter = "remote::Remote::into_inner", getter_owned)]
        inner: [u8; 4],
    }

    #[derive(Archive, ArchiveWith)]
    #[archive_with(from(remote::Remote))]
    struct ExampleByRef {
        #[archive_with(getter = "remote::Remote::to_inner")]
        inner: [u8; 4],
    }

    #[derive(Archive, ArchiveWith)]
    #[archive_with(from(remote::Remote))]
    struct ExampleThroughRef {
        #[archive_with(getter = "remote::Remote::as_inner")]
        inner: [u8; 4],
    }

    let remote = remote::Remote::default();
    let _ = archive::<ExampleByVal, _>(&serialize::<ExampleByVal, _>(&remote));
    let _ = archive::<ExampleByRef, _>(&serialize::<ExampleByRef, _>(&remote));
    let _ = archive::<ExampleThroughRef, _>(&serialize::<ExampleThroughRef, _>(&remote));
}

#[test]
fn unnamed_struct_private() {
    mod remote {
        #[derive(Copy, Clone, Default)]
        pub struct Remote([u8; 4]);

        impl Remote {
            pub fn into_inner(self) -> [u8; 4] {
                self.0
            }

            pub fn as_inner(&self) -> [u8; 4] {
                self.0
            }
        }
    }

    #[derive(Archive, ArchiveWith)]
    #[archive_with(from(remote::Remote))]
    struct ExampleByRef(#[archive_with(getter = "remote::Remote::as_inner")] [u8; 4]);

    #[derive(Archive, ArchiveWith)]
    #[archive_with(from(remote::Remote))]
    struct ExampleByVal(
        #[archive_with(getter = "remote::Remote::into_inner", getter_owned)] [u8; 4],
    );

    let remote = remote::Remote::default();
    let _ = archive::<ExampleByRef, _>(&serialize::<ExampleByRef, _>(&remote));
    let _ = archive::<ExampleByVal, _>(&serialize::<ExampleByVal, _>(&remote));
}
