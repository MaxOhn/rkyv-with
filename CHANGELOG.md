# v0.1.2 (2023-09-25)

- The function specified with `archive_with(getter = "...")` may now return a reference of the field's type

## v0.1.1 (2023-05-29)

- Fixed `DeserializeWith` impl when certain wrappers were used ([#2])
- A specified getter method for unnamed struct fields is now considered properly ([#4])

## v0.1.0 (2023-03-30)

First release

[#2]: https://github.com/MaxOhn/rkyv-with/pull/2
[#4]: https://github.com/MaxOhn/rkyv-with/pull/4