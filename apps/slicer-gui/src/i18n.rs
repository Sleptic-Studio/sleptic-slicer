// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Translating strings written in Rust, through the same catalogue Slint
//! uses for the `.slint` files.
//!
//! ```ignore
//! tr!("Hello world")
//! tr!("Hello {}", name)
//! tr!("One item", "{n} items", count)
//! tr!("One {} item", "{n} {} items", count, kind)
//! ctr!("MenuBar", "File")               // with a context (msgctxt)
//! ctr!("MenuBar", "One file", "{n} files", count)
//! ```
//!
//! For xgettext: `--keyword='tr!:1' --keyword='tr!:1,2'
//! --keyword='ctr!:1c,2' --keyword='ctr!:1c,2,3'`.
//!
//! A literal right after the message, numbers included, is read as the
//! plural form: `tr!("{} of {}", 3, n)` is a plural. Pass such values as
//! variables or other expressions.

/// One call into Slint's translator. `$args` become the `{}` values.
macro_rules! translate {
    ($context:expr, $msgid:literal, $plural:expr, $n:expr, [$($arg:expr),*]) => {{
        let args: &[slint::SharedString] = &[$(slint::SharedString::from(std::format!("{}", $arg))),*];
        slint::private_unstable_api::translate(
            $msgid.into(),
            $context,
            slint::SharedString::default(),
            slint::private_unstable_api::re_exports::Slice::from_slice(args),
            $n,
            $plural,
        )
    }};
}

/// Translates a message; see the module documentation for its forms.
#[allow(unused_macros)]
macro_rules! tr {
    ($one:literal, $many:literal, $n:expr, $($arg:expr),+ $(,)?) => {
        $crate::i18n::translate!(slint::SharedString::default(), $one, $many.into(), $n as i32, [$($arg),+])
    };
    ($one:literal, $many:literal, $n:expr $(,)?) => {
        $crate::i18n::translate!(slint::SharedString::default(), $one, $many.into(), $n as i32, [])
    };
    ($msgid:literal, $($arg:expr),+ $(,)?) => {
        $crate::i18n::translate!(slint::SharedString::default(), $msgid, slint::SharedString::default(), 0_i32, [$($arg),+])
    };
    ($msgid:literal $(,)?) => {
        $crate::i18n::translate!(slint::SharedString::default(), $msgid, slint::SharedString::default(), 0_i32, [])
    };
}

/// Like [`tr!`], with a context first that tells apart identical strings
/// needing different translations.
macro_rules! ctr {
    ($context:literal, $one:literal, $many:literal, $n:expr, $($arg:expr),+ $(,)?) => {
        $crate::i18n::translate!($context.into(), $one, $many.into(), $n as i32, [$($arg),+])
    };
    ($context:literal, $one:literal, $many:literal, $n:expr $(,)?) => {
        $crate::i18n::translate!($context.into(), $one, $many.into(), $n as i32, [])
    };
    ($context:literal, $msgid:literal, $($arg:expr),+ $(,)?) => {
        $crate::i18n::translate!($context.into(), $msgid, slint::SharedString::default(), 0_i32, [$($arg),+])
    };
    ($context:literal, $msgid:literal $(,)?) => {
        $crate::i18n::translate!($context.into(), $msgid, slint::SharedString::default(), 0_i32, [])
    };
}

#[allow(unused_imports)]
pub(crate) use {ctr, tr, translate};

#[cfg(test)]
mod tests {
    #[test]
    fn untranslated_messages_come_back_with_their_values_filled_in() {
        assert_eq!(tr!("Plain"), "Plain");
        let (three, four) = (3, "four");
        assert_eq!(tr!("{} of {}", three, four), "3 of four");
        assert_eq!(ctr!("Dialog", "Plain"), "Plain");
        let file = "a.stl";
        assert_eq!(ctr!("Dialog", "Open {}", file), "Open a.stl");
        // Plurals choose by the count; English: one, else many.
        assert_eq!(tr!("One item", "{n} items", 1), "One item");
        assert_eq!(tr!("One item", "{n} items", 4), "4 items");
        let red = "red";
        assert_eq!(tr!("One {} part", "{n} {} parts", 2, red), "2 red parts");
        assert_eq!(ctr!("Dialog", "One file", "{n} files", 0), "0 files");
        assert_eq!(
            ctr!("Dialog", "One {} file", "{n} {} files", 1, "new"),
            "One new file"
        );
        // A trailing comma is accepted in every form.
        assert_eq!(tr!("Plain",), "Plain");
        let seven = 7;
        assert_eq!(ctr!("Dialog", "{}", seven,), "7");
    }
}
