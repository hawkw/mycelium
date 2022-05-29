#[macro_export]
macro_rules! bitfield {
    (
        $(#[$($meta:meta)+])*
        $vis:vis struct $Name:ident<$T:ident> {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis const $Field:ident $(: $F:ty)? $( = $val:literal)?;
            )+
        }
    ) => {

        $(#[$($meta)+])*
        #[derive(Copy, Clone)]
        #[repr(transparent)]
        $vis struct $Name($T);

        #[automatically_derived]
        impl core::fmt::Debug for $Name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                if f.alternate() {
                    f.debug_tuple(stringify!($Name)).field(&format_args!("{}", self)).finish()
                } else {
                    f.debug_tuple(stringify!($Name)).field(&format_args!("{:#b}", self.0)).finish()
                }

            }
        }

        // Some generated methods may not always be used, which may emit dead
        // code warnings if the type is private.
        #[allow(dead_code)]
        #[automatically_derived]
        impl $Name {
            $crate::bitfield! { @field<$T>:
                $(
                    $(#[$field_meta])*
                    $field_vis const $Field $(: $F)? $( = $val)?;
                )+
            }

            const FIELDS: &'static [(&'static str, $crate::bitfield! { @t $T, $T })] = &[$(
                (stringify!($Field), Self::$Field.typed())
            ),+];

            $vis const fn from_bits(bits: $T) -> Self {
                Self(bits)
            }

            $vis const fn new() -> Self {
                Self(0)
            }

            $vis fn set<T>(self, packer: $crate::bitfield! { @t $T, T }, value: T) -> Self
            where
                T: $crate::bits::FromBits<$T>,
            {
                Self(packer.pack(value, self.0))
            }

            $vis fn get<T>(self, packer: $crate::bitfield! { @t $T, T }) -> T
            where
                T: $crate::bits::FromBits<$T>,
            {
                packer.unpack(self.0)
            }

            $vis fn try_get<T>(self, packer: $crate::bitfield! { @t $T, T }) -> Result<T, T::Error>
            where
                T: $crate::bits::FromBits<$T>,
            {
                packer.try_unpack(self.0)
            }


            $vis fn assert_valid() {
                <$crate::bitfield! { @t $T, $T }>::assert_all_valid(&Self::FIELDS);
            }
        }

        #[automatically_derived]
        impl core::fmt::Display for $Name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.pad("")?;
                writeln!(f, "{:0width$b}", self.0, width = $T::BITS as usize)?;
                f.pad("")?;
                let mut cur_pos = $T::BITS;
                let mut max_len = 0;
                let mut rem = 0;
                let mut fields = Self::FIELDS.iter().rev().peekable();
                while let Some((name, field)) = fields.next() {
                    while cur_pos > field.most_significant_index() {
                        f.write_str(" ")?;
                        cur_pos -= 1;
                    }
                    let bits = field.bits();
                    match (name, bits) {
                        (name, bits) if name.starts_with("_") => {
                            for _ in 0..bits {
                                f.write_str(" ")?;
                            }
                            cur_pos -= bits;
                            continue;
                        }
                        (_, 1) => f.write_str("│")?,
                        (_, 2) => f.write_str("└┤")?,
                        (_, bits) => {
                            f.write_str("└┬")?;
                            for _ in 0..(bits - 3) {
                                f.write_str("─")?;
                            }
                            f.write_str("┘")?;
                        }
                    }

                    if fields.peek().is_none() {
                        rem = cur_pos - (bits - 1);
                    }

                    max_len = core::cmp::max(max_len, name.len());
                    cur_pos -= field.bits()
                }

                f.write_str("\n")?;

                $(
                    let field = Self::$Field;
                    let name = stringify!($Field);
                    if !name.starts_with("_") {
                        f.pad("")?;
                        cur_pos = $T::BITS;
                        for (cur_name, cur_field) in Self::FIELDS.iter().rev() {
                            while cur_pos > cur_field.most_significant_index() {
                                f.write_str(" ")?;
                                cur_pos -= 1;
                            }

                            if field.typed::<$T>() == cur_field.typed::<$T>() {
                                break;
                            }

                            let bits = cur_field.bits();
                            match (cur_name, bits) {
                                (name, bits) if name.starts_with("_") => {
                                    for _ in 0..bits {
                                        f.write_str(" ")?;
                                    }
                                }
                                (_, 1) => f.write_str("│")?,
                                (_, bits) => {
                                    f.write_str(" │")?;
                                    for _ in 0..(bits - 2) {
                                        f.write_str(" ")?;
                                    }
                                }
                            }

                            cur_pos -= bits;
                        }

                        let field_bits = field.bits();
                        if field_bits == 1 {
                            f.write_str("└")?;
                            cur_pos -= 1;
                        } else {
                            f.write_str(" └")?;
                            cur_pos -= 2;
                        }
                        let len = cur_pos as usize + (max_len - name.len());
                        for _ in rem as usize..len {
                            f.write_str("─")?;
                        }
                        writeln!(f, " {}: {:?} ({:0width$b})", name, field.unpack(self.0), field.unpack_bits(self.0), width = field_bits as usize)?
                    }

                )+

                Ok(())
            }
        }

        #[automatically_derived]
        impl core::fmt::Binary for $Name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                if f.alternate() {
                    f.debug_tuple(stringify!($Name)).field(&format_args!("{:#b}", self)).finish()
                } else {
                    f.debug_tuple(stringify!($Name)).field(&format_args!("{:b}", self)).finish()
                }
            }
        }
    };

    (@field<$T:ident>, prev: $Prev:ident:
        $(#[$meta:meta])*
        $vis:vis const $Field:ident = $value:literal;
        $($rest:tt)*
    ) => {
        $(#[$meta])*
        $vis const $Field: $crate::bitfield!{ @t $T, $T } = Self::$Prev.next($value);
        $crate::bitfield!{ @field<$T>, prev: $Field: $($rest)* }
    };

    (@field<$T:ident>, prev: $Prev:ident:
        $(#[$meta:meta])*
        $vis:vis const $Field:ident: $Val:ty;
        $($rest:tt)*
    ) => {
        $(#[$meta])*
        $vis const $Field: $crate::bitfield!{ @t $T, $Val } = Self::$Prev.then::<$Val>();
        $crate::bitfield!{ @field<$T>, prev: $Field: $($rest)* }
    };

    (@field<$T:ident>, prev: $Prev:ident: ) => {  };
    (@field<$T:ident>:
        $(#[$meta:meta])*
        $vis:vis const $Field:ident = $value:literal;
        $($rest:tt)*
    ) => {
        $(#[$meta])*
        $vis const $Field: $crate::bitfield!{ @t $T, $T } = <$crate::bitfield!{ @t $T, $T }>::least_significant($value);
        $crate::bitfield!{ @field<$T>, prev: $Field: $($rest)* }
    };

    (@field<$T:ident>:
        $(#[$meta:meta])*
        $vis:vis const $Field:ident: $Val:ty;
        $($rest:tt)*
    ) => {
        $(#[$meta])*
        $vis const $Field: $crate::bitfield!{ @t $T, $Val } = <$crate::bitfield!{ @t $T, $Val } >::first();
        $crate::bitfield!{ @field<$T>, prev: $Field: $($rest)* }
    };


    // (@process_meta $vis:vis struct $Name:ident<$T:ty> { $(#[$before:meta])* } #[derive($($Derive:path),+)] $(#[$after:meta])*) => {
    //     $crate::bitfield! { @process_derives $vis struct $Name<$T> { } $($Derive),+ { $(#[$before])* $(#[$after])* } }

    // };
    // (@process_meta $vis:vis struct $Name:ident<$T:ty> {  }) => {
    //     #[derive(Copy, Clone)]
    //     #[repr(transparent)]
    //     $vis struct $Name($T);
    // };
    // (@process_meta $vis:vis struct $Name:ident<$T:ty> { $(#[$before:meta])+ }) => {
    //     $(#[$before])*
    //     #[derive(Copy, Clone)]
    //     #[repr(transparent)]
    //     $vis struct $Name($T);
    // };
    // (@process_meta $vis:vis struct $Name:ident<$T:ty>  { $(#[$before:meta])* } #[$current:meta] $(#[$after:meta])*) => {
    //     $crate::bitfield! { @process_meta $vis struct $Name<$T> { $(#[$before])* #[$current] } $(#[$after])* }
    // };
    // (@process_derives $vis:vis struct $Name:ident<$T:ty> { Debug, } { $($rest:tt)* }) => {
    //     impl core::fmt::Debug for $Name {
    //         fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    //             if f.alternate() {
    //                 f.debug_tuple(stringify!($Name)).field(&format_args!("{}", self)).finish()
    //             } else {
    //                 f.debug_tuple(stringify!($Name)).field(&format_args!("{:#b}", self)).finish()
    //             }

    //         }
    //     }
    //     #[derive(Copy, Clone)]
    //     #[repr(transparent)]
    //     $($rest)*
    //     $vis struct $Name($T);
    // };

    // (@process_derives $vis:vis struct $Name:ident<$T:ty> { Debug, $($Before:tt),+ } $($After:tt),+ { $($rest:tt)* }) => {
    //     impl core::fmt::Debug for $Name {
    //         fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    //             if f.alternate() {
    //                 f.debug_tuple(stringify!($Name)).field(&format_args!("{}", self)).finish()
    //             } else {
    //                 f.debug_tuple(stringify!($Name)).field(&format_args!("{:#b}", self)).finish()
    //             }

    //         }
    //     }
    //     #[derive(Copy, Clone, $($Before),+ $($After),+)]
    //     #[repr(transparent)]
    //     $($rest)*
    //     $vis struct $Name($T);
    // };
    // (@process_derives $vis:vis struct $Name:ident<$T:ty> { Debug, $($Before:tt),+ } { $($rest:tt)* }) => {

    //     #[derive(Copy, Clone, $($Before),+)]
    //     #[repr(transparent)]
    //     $($rest)*
    //     $vis struct $Name($T);
    // };
    // (@process_derives $vis:vis struct $Name:ident<$T:ty> { $($Before:tt),+ $(,)? } { $($rest:tt)* }) => {
    //     #[derive($($Before),+)]
    //     #[derive(Copy, Clone)]
    //     #[repr(transparent)]
    //     $($rest)*
    //     $vis struct $Name($T);
    // };
    // (@process_derives $vis:vis struct $Name:ident<$T:ty> { $($Before:tt),* $(,)? } $Next:tt, $($After:tt),* { $($rest:tt)* }) => {
    //     $crate::bitfield! { @process_derives $vis struct $Name<$T> { $Next, $($Before),*  } $($After),* { $($rest)* } }
    // };
    // (@process_derives $vis:vis struct $Name:ident<$T:ty> { $($Before:tt),* } $Next:tt { $($rest:tt)* }) => {
    //     $crate::bitfield! { @process_derives $vis struct $Name<$T> { $Next, $($Before),* } { $($rest)* } }
    // };

    (@t usize, $V:ty) => { $crate::bits::PackUsize<$V> };
    (@t u64, $V:ty) => { $crate::bits::Pack64<$V> };
    (@t u32, $V:ty) => { $crate::bits::Pack32<$V> };
    (@t u16, $V:ty) => { $crate::bits::Pack16<$V> };
    (@t u8, $V:ty) => { $crate::bits::Pack8<$V> };
    (@t $T:ty, $V:ty) => { compile_error!(concat!("unsupported bitfield type `", stringify!($T), "`; expected one of `usize`, `u64`, `u32`, `u16`, or `u8`")) }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use crate::bits::FromBits;

    bitfield! {
        #[allow(dead_code)]
        struct TestBitfield<u32> {
            const HELLO = 4;
            const _RESERVED_1 = 3;
            const WORLD: bool;
            const HAVE: TestEnum;
            const LOTS = 5;
            const OF = 1;
            const FUN = 6;
        }
    }

    #[repr(u8)]
    #[derive(Debug)]
    enum TestEnum {
        Foo = 0b00,
        Bar = 0b01,
        Baz = 0b10,
        Qux = 0b11,
    }

    impl FromBits<u32> for TestEnum {
        const BITS: u32 = 2;
        type Error = core::convert::Infallible;

        fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
            Ok(match bits as u8 {
                bits if bits == Self::Foo as u8 => Self::Foo,
                bits if bits == Self::Bar as u8 => Self::Bar,
                bits if bits == Self::Baz as u8 => Self::Baz,
                bits if bits == Self::Qux as u8 => Self::Qux,
                bits => unreachable!("all patterns are covered: {:#b}", bits),
            })
        }

        fn into_bits(self) -> u32 {
            self as u8 as u32
        }
    }

    #[derive(Debug)]
    #[allow(dead_code)]
    struct TestDebug {
        value: usize,
        bits: TestBitfield,
    }

    #[test]
    fn test_bitfield_format() {
        let test_bitfield = TestBitfield::new()
            .set(TestBitfield::HELLO, 0b1001)
            .set(TestBitfield::WORLD, true)
            .set(TestBitfield::HAVE, TestEnum::Bar)
            .set(TestBitfield::LOTS, 0b11010)
            .set(TestBitfield::OF, 0)
            .set(TestBitfield::FUN, 9);
        println!("{}", test_bitfield);

        let test_debug = TestDebug {
            value: 42,
            bits: test_bitfield,
        };

        println!("test_debug(alt): {:#?}", test_debug);

        println!("test_debug: {:?}", test_debug)
    }

    #[test]
    fn macro_bitfield_valid() {
        TestBitfield::assert_valid();
    }
}
