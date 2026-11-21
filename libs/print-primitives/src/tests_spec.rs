// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_mesh_core::primitives::CoordAxis;
use serde::{Deserialize, Serialize};
use std::fmt;
use serde::de::DeserializeOwned;
use std::error::Error;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Field<T> {
    value: T,
}
fn decode<T: DeserializeOwned>(s: &str) -> Result<T, toml_edit::de::Error> {
    toml_edit::de::from_str::<Field<T>>(s).map(|v| v.value)
}
fn roundtrip<T: Serialize + DeserializeOwned + PartialEq + fmt::Debug>(v: T) {
    let field = Field { value: v };
    let text = toml_edit::ser::to_string(&field).unwrap();
    assert_eq!(toml_edit::de::from_str::<Field<T>>(&text).unwrap(), field);
}
fn reject<T: DeserializeOwned>(values: &[&str]) {
    for v in values {
        assert!(
            decode::<T>(&format!("value = {v}")).is_err(),
            "accepted {v}"
        );
    }
}
macro_rules! positive_tests {
    ($test:ident, $ty:ident, $error:ident, $example:expr, $display:literal, $diagnostic:literal) => {
        #[test]
        fn $test() {
            for v in [f64::from_bits(1), f64::MIN_POSITIVE, $example, f64::MAX] {
                let n = $ty::new(v).unwrap();
                assert_eq!(n.value().to_bits(), v.to_bits());
                assert_eq!(f64::from($ty::try_from(v).unwrap()), v);
                roundtrip(n);
            }
            for v in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let e = $ty::new(v).unwrap_err();
                assert_eq!(e.value.to_bits(), v.to_bits());
                assert!(e.source().is_none());
            }
            assert_eq!($ty::new($example).unwrap().to_string(), $display);
            assert_eq!($error { value: -1.0 }.to_string(), $diagnostic);
            assert_eq!(
                decode::<$ty>(&format!("value = {}", $example))
                    .unwrap()
                    .value(),
                $example
            );
            reject::<$ty>(&["0", "-0.0", "-1", "nan", "inf", "-inf", "\"1\"", "true"]);
        }
    };
}
positive_tests!(
    millimetres,
    PositiveFiniteMm,
    LengthOutOfRange,
    0.05,
    "0.05",
    "length must be strictly positive and finite, got -1"
);
positive_tests!(
    microns,
    PositiveFiniteMicrons,
    LengthOutOfRange,
    18.5,
    "18.5",
    "length must be strictly positive and finite, got -1"
);
positive_tests!(
    seconds,
    PositiveFiniteSec,
    SecOutOfRange,
    2.5,
    "2.5",
    "duration must be strictly positive and finite, got -1 s"
);
positive_tests!(
    speed,
    PositiveFiniteMmPerMin,
    SpeedMmPerMinOutOfRange,
    60.0,
    "60",
    "speed must be strictly positive and finite, got -1 mm/min"
);
positive_tests!(
    energy,
    PositiveFiniteMjPerCm2,
    MjPerCm2OutOfRange,
    15.5,
    "15.5",
    "energy density must be strictly positive and finite, got -1 mJ/cm^2"
);
positive_tests!(
    intensity,
    PositiveFiniteMwPerCm2,
    MwPerCm2OutOfRange,
    5.0,
    "5",
    "light intensity must be strictly positive and finite, got -1 mW/cm^2"
);
positive_tests!(
    price,
    PositiveFiniteCurrencyPerLiter,
    CurrencyPerLiterOutOfRange,
    34.99,
    "34.99",
    "price must be strictly positive and finite, got -1 per litre"
);

#[test]
fn length_parsing_table() {
    for (s, v) in [("0.05", 0.05), ("+1", 1.0), ("1e-3", 0.001)] {
        assert_eq!(s.parse::<PositiveFiniteMm>().unwrap().value(), v);
        assert_eq!(s.parse::<PositiveFiniteMicrons>().unwrap().value(), v);
    }
    for s in [" 1", "1 ", "", "abc", "1 mm"] {
        for e in [
            s.parse::<PositiveFiniteMm>().unwrap_err(),
            s.parse::<PositiveFiniteMicrons>().unwrap_err(),
        ] {
            assert!(matches!(e, ParseLengthError::NotANumber(_)));
            assert_eq!(
                e.to_string(),
                format!("invalid length value: {}", e.source().unwrap())
            );
        }
    }
    for s in ["0", "-0", "-1", "NaN", "inf", "1e999", "1e-999"] {
        for e in [
            s.parse::<PositiveFiniteMm>().unwrap_err(),
            s.parse::<PositiveFiniteMicrons>().unwrap_err(),
        ] {
            assert!(matches!(e, ParseLengthError::OutOfRange(_)));
            assert_eq!(e.to_string(), e.source().unwrap().to_string());
        }
    }
}

#[test]
fn ranges() {
    let lo = PositiveFiniteMm::new(0.01).unwrap();
    let hi = PositiveFiniteMm::new(0.15).unwrap();
    let r = LengthRange::new(lo, hi).unwrap();
    assert_eq!((r.min(), r.max()), (lo, hi));
    assert_eq!(
        decode::<LengthRange>("value = { min = 0.01, max = 0.15 }").unwrap(),
        r
    );
    roundtrip(r);
    assert_eq!(
        LengthRange::new(lo, lo).unwrap_err().to_string(),
        "length range min (0.01 mm) is not strictly less than max (0.01 mm)"
    );
    assert!(LengthRange::new(hi, lo).is_err());
    reject::<LengthRange>(&[
        "{ min = 1, max = 1 }",
        "{ min = 2, max = 1 }",
        "{ min = 0, max = 1 }",
        "{ min = 1 }",
        "{ max = 2 }",
    ]);
    let lo = PositiveFiniteMmPerMin::new(30.0).unwrap();
    let hi = PositiveFiniteMmPerMin::new(300.0).unwrap();
    let r = SpeedRangeMmPerMin::new(lo, hi).unwrap();
    assert_eq!((r.min(), r.max()), (lo, hi));
    assert_eq!(
        decode::<SpeedRangeMmPerMin>("value = { min = 30.0, max = 300.0 }").unwrap(),
        r
    );
    roundtrip(r);
    assert_eq!(
        SpeedRangeMmPerMin::new(lo, lo).unwrap_err().to_string(),
        "speed range min (30 mm/min) is not strictly less than max (30 mm/min)"
    );
    assert!(SpeedRangeMmPerMin::new(hi, lo).is_err());
    reject::<SpeedRangeMmPerMin>(&[
        "{ min = 1, max = 1 }",
        "{ min = 2, max = 1 }",
        "{ min = -1, max = 1 }",
        "{ min = 1 }",
        "{ max = 2 }",
    ]);
}

#[test]
fn dimensions_and_axis_errors() {
    let v = BuildVolume::new(192.0, 120.0, 200.0).unwrap();
    assert_eq!(
        [v.x_mm(), v.y_mm(), v.z_mm()],
        [v.x().value(), v.y().value(), v.z().value()]
    );
    assert_eq!(v.envelope_min_mm(), [-96.0, -60.0, 0.0]);
    assert_eq!(v.envelope_min_mm()[2].to_bits(), 0.0f64.to_bits());
    assert_eq!(v.envelope_max_mm(), [96.0, 60.0, 200.0]);
    assert_eq!(
        decode::<BuildVolume>("value = { x_mm = 192.0, y_mm = 120.0, z_mm = 200.0 }").unwrap(),
        v
    );
    roundtrip(v);
    for (xyz, axis) in [
        ([0.0, 0.0, 0.0], CoordAxis::X),
        ([1.0, 0.0, 0.0], CoordAxis::Y),
        ([1.0, 1.0, 0.0], CoordAxis::Z),
    ] {
        let e = BuildVolume::new(xyz[0], xyz[1], xyz[2]).unwrap_err();
        assert_eq!(e.axis, axis);
        assert_eq!(
            e.to_string(),
            format!(
                "build volume {axis} axis must be a finite positive length in millimetres, got 0"
            )
        );
        assert_eq!(e.source().unwrap().to_string(), e.source.to_string());
    }
    let tiny = BuildVolume::new(f64::from_bits(1), f64::MAX, 1.0).unwrap();
    assert_eq!(tiny.envelope_max_mm()[0], 0.0);
    reject::<BuildVolume>(&[
        "{}",
        "{ x_mm = 1, y_mm = 1 }",
        "{ x_mm = 0, y_mm = 1, z_mm = 1 }",
    ]);
    for (x, y) in [(1, 1), (u32::MAX, u32::MAX), (1920, 1080)] {
        let p = PixelResolution::new(x, y).unwrap();
        assert_eq!((p.x(), p.y()), (x, y));
        roundtrip(p);
    }
    for (x, y, axis) in [(0, 0, CoordAxis::X), (1, 0, CoordAxis::Y)] {
        let e = PixelResolution::new(x, y).unwrap_err();
        assert_eq!(e.axis, axis);
        assert_eq!(
            e.to_string(),
            format!("pixel resolution {axis} axis must be non-zero")
        );
        assert!(e.source().is_none());
    }
    assert_eq!(
        decode::<PixelResolution>("value = { x = 1920, y = 1080 }").unwrap(),
        PixelResolution::new(1920, 1080).unwrap()
    );
    reject::<PixelResolution>(&[
        "{}",
        "{ x = 1 }",
        "{ x = -1, y = 1 }",
        "{ x = 1.5, y = 1 }",
        "{ x = 4294967296, y = 1 }",
        "{ x = 0, y = 1 }",
    ]);
    let p = PixelSizeMicrons::new(34.0, 51.0).unwrap();
    assert_eq!((p.x.value(), p.y.value()), (34.0, 51.0));
    assert_eq!(
        decode::<PixelSizeMicrons>("value = { x = 34.0, y = 51.0 }").unwrap(),
        p
    );
    roundtrip(p);
    for (x, y, axis) in [(0.0, 0.0, PixelAxis::X), (1.0, 0.0, PixelAxis::Y)] {
        let e = PixelSizeMicrons::new(x, y).unwrap_err();
        assert_eq!(e.axis, axis);
        assert_eq!(
            e.to_string(),
            format!("pixel size {axis} axis must be a finite positive length in microns, got 0")
        );
        assert_eq!(e.source().unwrap().to_string(), e.source.to_string());
    }
    assert_eq!(PixelAxis::X.to_string(), "x");
    assert_eq!(PixelAxis::Y.to_string(), "y");
    reject::<PixelSizeMicrons>(&["{}", "{ x = 1 }", "{ x = 1, y = inf }"]);
}

#[test]
fn mirrors_and_aa() {
    assert_eq!(Mirror::default(), Mirror { x: false, y: false });
    for x in [false, true] {
        for y in [false, true] {
            roundtrip(Mirror { x, y });
        }
    }
    assert_eq!(
        decode::<Mirror>("value = { x = true, y = false }").unwrap(),
        Mirror { x: true, y: false }
    );
    reject::<Mirror>(&["{}", "{ x = true }", "{ x = 1, y = false }"]);
    for (s, a) in [
        ("off", AaLevel::Off),
        ("x2", AaLevel::X2),
        ("x4", AaLevel::X4),
        ("x8", AaLevel::X8),
    ] {
        assert_eq!(s.parse::<AaLevel>().unwrap(), a);
        assert_eq!(decode::<AaLevel>(&format!("value = {s:?}")).unwrap(), a);
        roundtrip(a);
    }
    for s in ["", "4", "X4", " x4", "x4 ", "a\"\n"] {
        let e = s.parse::<AaLevel>().unwrap_err();
        assert_eq!(e.got, s);
        assert_eq!(
            e.to_string(),
            format!("unknown AA level \"{s}\"; expected one of: off, x2, x4, x8")
        );
    }
    reject::<AaLevel>(&["4", "\"X4\"", "\" x4\""]);
}

#[test]
fn identifiers_and_nonempty_strings() {
    for s in ["a", "3", "printer-1", "a--b", "foo-", "standard-405"] {
        let p = PrinterId::new(s).unwrap();
        assert_eq!(p.as_str(), s);
        assert_eq!(p.to_string(), s);
        assert_eq!(p, s.parse().unwrap());
        assert_eq!(String::from(PrinterId::try_from(s.to_owned()).unwrap()), s);
        roundtrip(p);
        let m = MaterialId::new(s).unwrap();
        assert_eq!(m.as_str(), s);
        assert_eq!(m.to_string(), s);
        assert_eq!(m, s.parse().unwrap());
        assert_eq!(String::from(MaterialId::try_from(s.to_owned()).unwrap()), s);
        roundtrip(m);
    }
    for s in [
        "",
        "-A",
        "A",
        "a_b",
        "a.b",
        "a/b",
        "a b",
        "caf\u{e9}",
        "a\0",
    ] {
        let p = PrinterId::new(s).unwrap_err();
        assert_eq!(MaterialId::new(s).unwrap_err(), p);
        let expected = if s.is_empty() {
            InvalidProfileId::Empty
        } else if s.starts_with('-') {
            InvalidProfileId::LeadingHyphen {
                value: s.to_owned(),
            }
        } else {
            InvalidProfileId::InvalidCharAt {
                value: s.to_owned(),
                index: if s == "caf\u{e9}" {
                    3
                } else if s == "A" {
                    0
                } else {
                    1
                },
            }
        };
        assert_eq!(p, expected);
        assert!(p.source().is_none());
    }
    assert_eq!(
        InvalidProfileId::Empty.to_string(),
        "profile id must not be empty"
    );
    assert_eq!(
        PrinterId::new("-A").unwrap_err().to_string(),
        "profile id \"-A\" must not start with a hyphen"
    );
    assert_eq!(
        PrinterId::new("a\n").unwrap_err().to_string(),
        "profile id \"a\\n\" has an invalid character at byte 1; allowed characters are [a-z0-9-]"
    );
    assert!(PrinterId::new("a").unwrap() < PrinterId::new("b").unwrap());
    assert!(MaterialId::new("a").unwrap() < MaterialId::new("b").unwrap());
    let mut map = std::collections::BTreeMap::new();
    map.insert(
        PrinterId::new("printer-1").unwrap(),
        MaterialId::new("standard-405").unwrap(),
    );
    roundtrip(map);
    reject::<PrinterId>(&["\"\"", "\"A\"", "1"]);
    reject::<MaterialId>(&["\"\"", "\"A\"", "1"]);
    for s in ["Grey resin", " ", "\n", "\u{e9}", "a\0b"] {
        let n = NonEmptyString::new(s).unwrap();
        assert_eq!(n.as_str(), s);
        assert_eq!(n.to_string(), s);
        assert_eq!(n, s.parse().unwrap());
        assert_eq!(
            String::from(NonEmptyString::try_from(s.to_owned()).unwrap()),
            s
        );
        roundtrip(n);
    }
    assert_eq!(NonEmptyString::new("").unwrap_err(), EmptyStringError);
    assert_eq!(EmptyStringError.to_string(), "value must not be empty");
    reject::<NonEmptyString>(&["\"\"", "1"]);
}

#[test]
fn compensation() {
    for v in [-10.0, -2.5, -0.0, 0.0, 10.0] {
        let n = ScaleCompensationPercent::new(v).unwrap();
        assert_eq!(n.value().to_bits(), v.to_bits());
        assert_eq!(
            f64::from(ScaleCompensationPercent::try_from(v).unwrap()).to_bits(),
            v.to_bits()
        );
        roundtrip(n);
    }
    for v in [
        -10.00001,
        10.00001,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        let e = ScaleCompensationPercent::new(v).unwrap_err();
        assert_eq!(e.value.to_bits(), v.to_bits());
        assert_eq!((e.min, e.max), (-10.0, 10.0));
    }
    assert_eq!(
        ScaleCompensationPercent::new(-2.5).unwrap().to_string(),
        "-2.5"
    );
    assert_eq!(
        ScaleCompensationPercentOutOfBound {
            value: 4.0,
            min: 1.0,
            max: 3.0
        }
        .to_string(),
        "scale compensation must be finite and in [1%, 3%], got 4%"
    );
    roundtrip(decode::<ScaleCompensation>("value = { x = 0.0, y = 0.5, z = -1.0 }").unwrap());
    reject::<ScaleCompensationPercent>(&["nan", "inf", "-inf", "10.1", "-10.1", "\"1\""]);
    reject::<ScaleCompensation>(&["{}", "{ x = 0, y = 0 }", "{ x = 0, y = 0, z = 11 }"]);
}

#[test]
fn canonical_strings_and_bindings() {
    for s in [
        "sl1",
        "ctb",
        "goo",
        "pwmx",
        "png_per_layer",
        "svg_per_layer",
        "svg_combined",
        "future_format",
        "",
        "CTB",
        " ctb ",
        "\u{e9}",
    ] {
        let f = FormatKind::from_canonical(s);
        assert_eq!(f.as_canonical(), s);
        assert_eq!(decode::<FormatKind>(&format!("value = {s:?}")).unwrap(), f);
        roundtrip(f);
    }
    assert_ne!(FormatKind::Unknown("ctb".into()), FormatKind::Ctb);
    let text = toml_edit::ser::to_string(&Field {
        value: FormatKind::Unknown("ctb".into()),
    })
    .unwrap();
    assert_eq!(decode::<FormatKind>(&text).unwrap(), FormatKind::Ctb);
    for s in [
        "elegoo_centauri",
        "anycubic_homie",
        "moonraker",
        "sdcp",
        "future_protocol",
        "",
        "SDCP",
        " sdcp ",
        "\u{e9}",
    ] {
        let n = NetworkProtocol::from_canonical(s);
        assert_eq!(n.as_canonical(), s);
        assert_eq!(
            decode::<NetworkProtocol>(&format!("value = {s:?}")).unwrap(),
            n
        );
        roundtrip(n);
    }
    assert_ne!(
        NetworkProtocol::Unknown("sdcp".into()),
        NetworkProtocol::Sdcp
    );
    let text = toml_edit::ser::to_string(&Field {
        value: NetworkProtocol::Unknown("sdcp".into()),
    })
    .unwrap();
    assert_eq!(
        decode::<NetworkProtocol>(&text).unwrap(),
        NetworkProtocol::Sdcp
    );
    reject::<FormatKind>(&["1", "true"]);
    reject::<NetworkProtocol>(&["1", "true"]);
    for s in [
        "{ format = \"ctb\", version = \"4\" }",
        "{ format = \"goo\", version = \"V3.0\" }",
        "{ format = \"png_per_layer\" }",
        "{ format = \"\", version = \" \" }",
    ] {
        roundtrip(decode::<OutputFormatBinding>(&format!("value = {s}")).unwrap());
    }
    let b = OutputFormatBinding {
        format: FormatKind::Sl1,
        version: None,
    };
    let text = toml_edit::ser::to_string(&Field { value: b.clone() }).unwrap();
    assert!(!text.contains("version"));
    let list: nonempty_collections::NEVec<OutputFormatBinding> =
        OutputFormatBindings::new(b.clone());
    assert_eq!(list.first(), &b);
    assert_eq!(list.len().get(), 1);
    #[cfg(feature = "serde")]
    roundtrip(list);
    #[cfg(feature = "serde")]
    let list = decode::<OutputFormatBindings>("value = [{ format = \"sl1\" }, { format = \"ctb\", version = \"4\" }, { format = \"sl1\" }]").unwrap();
    #[cfg(feature = "serde")]
    assert_eq!(list.len().get(), 3);
    #[cfg(feature = "serde")]
    roundtrip(list);
    reject::<OutputFormatBinding>(&[
        "{}",
        "{ format = \"ctb\", version = \"\" }",
        "{ format = \"ctb\", version = 4 }",
    ]);
    #[cfg(feature = "serde")]
    reject::<OutputFormatBindings>(&["[]", "[{}]"]);
}

#[test]
fn rotation_and_calibration() {
    for (v, r, swaps) in [
        (0, LcdRotation::Deg0, false),
        (90, LcdRotation::Deg90, true),
        (180, LcdRotation::Deg180, false),
        (270, LcdRotation::Deg270, true),
    ] {
        assert_eq!(LcdRotation::try_from(v).unwrap(), r);
        assert_eq!(u16::from(r), v);
        assert_eq!(r.swaps_axes(), swaps);
        assert_eq!(decode::<LcdRotation>(&format!("value = {v}")).unwrap(), r);
        roundtrip(r);
    }
    for v in [1, 89, 360, u16::MAX] {
        assert_eq!(
            LcdRotation::try_from(v).unwrap_err(),
            "LCD rotation must be 0, 90, 180 or 270 degrees"
        );
    }
    reject::<LcdRotation>(&["-1", "65536", "90.0", "\"90\"", "\"Deg90\"", "360"]);
    assert_eq!(LcdRotation::default(), LcdRotation::Deg0);
    let d = MachineCalibration::default();
    assert!(d.is_default());
    assert_eq!(d.xy_scale(), [1.0, 1.0]);
    assert_eq!(decode::<MachineCalibration>("value = {}").unwrap(), d);
    assert_eq!(
        decode::<MachineCalibration>("value = { rotation = 90 }")
            .unwrap()
            .xy_scale(),
        [1.0, 1.0]
    );
    assert_eq!(
        decode::<MachineCalibration>("value = { xy_scale = [1.01, 0.99] }")
            .unwrap()
            .rotation,
        LcdRotation::Deg0
    );
    let c =
        decode::<MachineCalibration>("value = { rotation = 90, xy_scale = [1.01, 0.99] }").unwrap();
    assert!(!c.is_default());
    roundtrip(c);
    roundtrip(d);
    let text = toml_edit::ser::to_string(&Field { value: d }).unwrap();
    assert!(text.contains("rotation") && text.contains("xy_scale"));
    for scales in [[0.5, 2.0], [2.0, 0.5]] {
        roundtrip(MachineCalibration::new(LcdRotation::Deg180, scales).unwrap());
    }
    for v in [
        0.499999,
        2.000001,
        0.0,
        -0.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        for scales in [[v, 1.0], [1.0, v]] {
            assert_eq!(
                MachineCalibration::new(LcdRotation::Deg0, scales).unwrap_err(),
                "XY scale correction must be between 0.5 and 2"
            );
        }
    }
    assert!(
        !MachineCalibration::new(LcdRotation::Deg0, [1.0 + f64::EPSILON, 1.0])
            .unwrap()
            .is_default()
    );
    let m = d.measured([100.0, 100.0], [99.0, 101.0]).unwrap();
    assert_eq!(m.xy_scale(), [100.0 / 99.0, 100.0 / 101.0]);
    let c = MachineCalibration::new(LcdRotation::Deg270, [1.2, 0.8]).unwrap();
    let m = c.measured([99.0, 101.0], [100.0, 100.0]).unwrap();
    assert_eq!(m.rotation, LcdRotation::Deg270);
    assert_eq!(m.xy_scale(), [1.2 * 99.0 / 100.0, 0.8 * 101.0 / 100.0]);
    for v in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for i in 0..4 {
            let mut lengths = [1.0; 4];
            lengths[i] = v;
            assert_eq!(
                d.measured([lengths[0], lengths[1]], [lengths[2], lengths[3]])
                    .unwrap_err(),
                "Ruler lengths must be finite and positive"
            );
        }
    }
    for (n, m) in [
        (f64::MAX, f64::from_bits(1)),
        (f64::from_bits(1), f64::MAX),
        (3.0, 1.0),
    ] {
        assert_eq!(
            d.measured([n, 1.0], [m, 1.0]).unwrap_err(),
            "XY scale correction must be between 0.5 and 2"
        );
    }
    // Multiplication must happen before division, even when its intermediate overflows.
    assert!(
        MachineCalibration::new(LcdRotation::Deg0, [2.0, 1.0])
            .unwrap()
            .measured([f64::MAX, 1.0], [f64::MAX, 1.0])
            .is_err()
    );
    reject::<MachineCalibration>(&[
        "{ other = 1 }",
        "{ xy_scale = [1] }",
        "{ xy_scale = [1, 1, 1] }",
        "{ xy_scale = [1, 2.1] }",
        "{ xy_scale = [nan, 1] }",
        "{ rotation = 360 }",
    ]);
}

#[test]
fn object_field_contracts() {
    assert!(decode::<Mirror>("value = { x = true, y = false, extra = 1 }").is_ok());
    assert!(decode::<BuildVolume>("value = { x_mm = 1, y_mm = 2, z_mm = 3, extra = 1 }").is_ok());
    assert!(decode::<PixelResolution>("value = { x = 1, y = 2, extra = 1 }").is_ok());
    assert!(decode::<LengthRange>("value = { min = 1, max = 2, extra = 1 }").is_ok());
    assert!(decode::<SpeedRangeMmPerMin>("value = { min = 1, max = 2, extra = 1 }").is_ok());
    assert!(decode::<PixelSizeMicrons>("value = { x = 1, y = 2, extra = 1 }").is_ok());
    assert!(decode::<ScaleCompensation>("value = { x = 0, y = 0, z = 0, extra = 1 }").is_ok());
    assert!(decode::<OutputFormatBinding>("value = { format = \"sl1\", extra = 1 }").is_ok());
    reject::<Mirror>(&["{ x = true, x = false, y = true }"]);
    reject::<MachineCalibration>(&["{ rotation = 0, rotation = 90 }"]);
}
