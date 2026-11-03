//! The EQ's six bands (PLG-062): the parameters each band reads, and the filter types it can
//! take. Band 1 is the old low cut, 2 the low shelf, 3 and 4 the bells, 5 the high shelf and
//! 6 the high cut; their old parameters keep their ids, the new ones follow `output`.

/// The filter a band is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BandType {
    LowCut,
    LowShelf,
    Peaking,
    Notch,
    HighShelf,
    HighCut,
}

impl BandType {
    pub const ALL: [BandType; 6] = [
        BandType::LowCut,
        BandType::LowShelf,
        BandType::Peaking,
        BandType::Notch,
        BandType::HighShelf,
        BandType::HighCut,
    ];

    /// The choice's label, as a band's type parameter shows it.
    pub const fn label(self) -> &'static str {
        match self {
            BandType::LowCut => "Low Cut",
            BandType::LowShelf => "Low Shelf",
            BandType::Peaking => "Peaking",
            BandType::Notch => "Notch",
            BandType::HighShelf => "High Shelf",
            BandType::HighCut => "High Cut",
        }
    }

    /// Whether the band's gain moves the sound: a shelf's and a bell's; a cut and a notch
    /// ignore it.
    pub const fn has_gain(self) -> bool {
        matches!(
            self,
            BandType::LowShelf | BandType::Peaking | BandType::HighShelf
        )
    }

    /// Whether the band's Q moves the sound: every type's. A bell's and a notch's is their
    /// width; a shelf's and a cut's is the bump at the corner (1/√2 has none: a slope-1
    /// shelf, a Butterworth cut).
    pub const fn has_q(self) -> bool {
        true
    }

    /// Whether the band is a cut, the one type with a slope.
    pub const fn is_cut(self) -> bool {
        matches!(self, BandType::LowCut | BandType::HighCut)
    }
}

/// The types bands 1 and 2 take.
pub const LOW_TYPES: &[BandType] = &[
    BandType::LowCut,
    BandType::LowShelf,
    BandType::Peaking,
    BandType::Notch,
];
/// The types bands 3 and 4 take: all six.
pub const MID_TYPES: &[BandType] = &BandType::ALL;
/// The types bands 5 and 6 take.
pub const HIGH_TYPES: &[BandType] = &[
    BandType::Peaking,
    BandType::Notch,
    BandType::HighShelf,
    BandType::HighCut,
];

/// The labels of [`LOW_TYPES`], [`MID_TYPES`] and [`HIGH_TYPES`], for the type parameters.
pub(crate) const LOW_TYPE_LABELS: &[&str] = &["Low Cut", "Low Shelf", "Peaking", "Notch"];
pub(crate) const MID_TYPE_LABELS: &[&str] = &[
    "Low Cut",
    "Low Shelf",
    "Peaking",
    "Notch",
    "High Shelf",
    "High Cut",
];
pub(crate) const HIGH_TYPE_LABELS: &[&str] = &["Peaking", "Notch", "High Shelf", "High Cut"];

/// One band of the EQ: the ids of its parameters in [`crate::EQ`] and the types it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EqBand {
    pub on: &'static str,
    /// The type: an index into `types`.
    pub kind: &'static str,
    pub freq: &'static str,
    pub gain: &'static str,
    pub q: &'static str,
    /// The slope of a cut ([`crate::SLOPES`]).
    pub slope: &'static str,
    pub types: &'static [BandType],
}

impl EqBand {
    /// The type of the type parameter's plain value (an index, rounded and kept in range).
    pub fn type_of(&self, index: f64) -> BandType {
        let last = self.types.len() - 1;
        let i = if index.is_nan() {
            0
        } else {
            (index.round().max(0.0) as usize).min(last)
        };
        self.types[i]
    }

    /// The type parameter's plain value of `kind`, if the band takes it.
    pub fn index_of(&self, kind: BandType) -> Option<usize> {
        self.types.iter().position(|t| *t == kind)
    }
}

/// The six bands, low to high.
pub const EQ_BANDS: [EqBand; 6] = [
    EqBand {
        on: "low_cut_on",
        kind: "b1_type",
        freq: "low_cut_freq",
        gain: "b1_gain",
        q: "b1_q",
        slope: "low_cut_slope",
        types: LOW_TYPES,
    },
    EqBand {
        on: "b2_on",
        kind: "b2_type",
        freq: "low_freq",
        gain: "low_gain",
        q: "b2_q",
        slope: "b2_slope",
        types: LOW_TYPES,
    },
    EqBand {
        on: "b3_on",
        kind: "b3_type",
        freq: "mid1_freq",
        gain: "mid1_gain",
        q: "mid1_q",
        slope: "b3_slope",
        types: MID_TYPES,
    },
    EqBand {
        on: "b4_on",
        kind: "b4_type",
        freq: "mid2_freq",
        gain: "mid2_gain",
        q: "mid2_q",
        slope: "b4_slope",
        types: MID_TYPES,
    },
    EqBand {
        on: "b5_on",
        kind: "b5_type",
        freq: "high_freq",
        gain: "high_gain",
        q: "b5_q",
        slope: "b5_slope",
        types: HIGH_TYPES,
    },
    EqBand {
        on: "high_cut_on",
        kind: "b6_type",
        freq: "high_cut_freq",
        gain: "b6_gain",
        q: "b6_q",
        slope: "high_cut_slope",
        types: HIGH_TYPES,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Curve, EQ, SLOPES};

    #[test]
    fn every_band_names_parameters_of_the_eq() {
        for (n, b) in EQ_BANDS.iter().enumerate() {
            for id in [b.on, b.kind, b.freq, b.gain, b.q, b.slope] {
                assert!(EQ.param(id).is_some(), "帯 {}: {id}", n + 1);
            }
            let (_, on) = EQ.param(b.on).unwrap();
            assert_eq!(on.curve, Curve::Toggle);
            let (_, slope) = EQ.param(b.slope).unwrap();
            assert_eq!(slope.curve, Curve::Choice(SLOPES));
            let (_, kind) = EQ.param(b.kind).unwrap();
            let labels: Vec<_> = b.types.iter().map(|t| t.label()).collect();
            assert_eq!(kind.choices(), &labels[..], "帯 {}", n + 1);
        }
    }

    #[test]
    fn the_default_types_are_the_old_bands() {
        let want = [
            BandType::LowCut,
            BandType::LowShelf,
            BandType::Peaking,
            BandType::Peaking,
            BandType::HighShelf,
            BandType::HighCut,
        ];
        for (b, t) in EQ_BANDS.iter().zip(want) {
            let (_, kind) = EQ.param(b.kind).unwrap();
            assert_eq!(b.type_of(kind.default), t);
        }
    }

    #[test]
    fn a_type_index_out_of_range_is_the_nearest_end() {
        let b = &EQ_BANDS[4];
        assert_eq!(b.type_of(-3.0), BandType::Peaking);
        assert_eq!(b.type_of(9.0), BandType::HighCut);
        assert_eq!(b.type_of(f64::NAN), BandType::Peaking);
        assert_eq!(b.index_of(BandType::HighShelf), Some(2));
        assert_eq!(b.index_of(BandType::LowCut), None);
    }
}
