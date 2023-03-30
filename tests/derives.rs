use std::{fmt::Debug, num::NonZeroU64};

use rkyv::{
    ser::serializers::AllocSerializer,
    with::{ArchiveWith, CopyOptimize, DeserializeWith, Niche, SerializeWith, With},
    Archive, Archived, Infallible,
};
use rkyv_with::{ArchiveWith, DeserializeWith};

fn roundtrip<Wrapper, Remote>(remote: &Remote)
where
    Wrapper: ArchiveWith<Remote>
        + SerializeWith<Remote, AllocSerializer<8>>
        + DeserializeWith<Archived<With<Remote, Wrapper>>, Remote, Infallible>,
    Remote: Debug + PartialEq,
{
    let with = With::<Remote, Wrapper>::cast(remote);
    let bytes = rkyv::to_bytes::<_, 8>(with).unwrap();
    let archived = unsafe { rkyv::archived_root::<With<Remote, Wrapper>>(&bytes) };
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
    }

    let remote = Remote {
        a: 0,
        b: Vec::new(),
        c: None,
    };

    roundtrip::<Example<i32>, _>(&remote);
}

#[test]
fn unnamed_struct() {
    #[derive(Debug, PartialEq)]
    struct Remote<A>(u8, Vec<A>, Option<NonZeroU64>);

    #[derive(Archive, ArchiveWith, DeserializeWith)]
    #[archive_with(from(Remote::<A>))]
    struct Example<A>(
        u8,
        #[with(CopyOptimize)] Vec<A>,
        #[with(Niche)]
        #[archive_with(from(Option<NonZeroU64>), via(Niche))]
        Option<NonZeroU64>,
    );

    let remote = Remote(0, Vec::new(), None);
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
        C { a: Vec<A>, b: Option<NonZeroU64> },
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
        },
    }

    for remote in [
        Remote::A,
        Remote::B(0),
        Remote::C {
            a: Vec::new(),
            b: None,
        },
    ] {
        roundtrip::<Example<i32>, _>(&remote);
    }
}
