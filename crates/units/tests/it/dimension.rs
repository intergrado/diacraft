//! `Quantity`'s dimension: the power of its unit.

use diacraft_units::{Quantity, Unit};

use crate::close;

#[test]
fn a_quantity_starts_at_the_dimension_its_unit_gives() {
    // A unit-free number has dimension 0, a single unit 1, and acres and hectares 2. Percent is 1.
    assert_eq!(Quantity::new(5.0, Unit::Unitless).dim(), 0);
    assert_eq!(Quantity::new(5.0, Unit::Inch).dim(), 1);
    assert_eq!(Quantity::new(5.0, Unit::Degree).dim(), 1);
    assert_eq!(Quantity::new(5.0, Unit::ElapsedHour).dim(), 1);
    assert_eq!(Quantity::new(5.0, Unit::Percent).dim(), 1);
    assert_eq!(Quantity::new(5.0, Unit::Date).dim(), 1);
    assert_eq!(Quantity::new(5.0, Unit::Acre).dim(), 2);
    assert_eq!(Quantity::new(5.0, Unit::Hectare).dim(), 2);
    assert_eq!(Quantity::from_value(3.0, Unit::Foot).dim(), 1);
}

#[test]
fn a_multidimensional_value_is_shown_in_the_unit_it_names_to_its_power() {
    // 2.54 cm * 2.54 cm is stored as one square inch and shows 6.4516 CM^2, not 1.
    let area = Quantity::with_dim(1.0, Unit::Centimetre, 2);
    assert_eq!(area.dim(), 2);
    assert!(close(area.shown(), 6.4516), "shown {}", area.shown());
    // The page's cube: 1 FT^3 is 1728 cubic inches.
    let cube = Quantity::with_dim(1728.0, Unit::Foot, 3);
    assert!(close(cube.shown(), 1.0), "shown {}", cube.shown());
    // A negative power divides.
    let per = Quantity::with_dim(0.5, Unit::Inch, -1);
    assert!(close(per.shown(), 0.5));
    let per_cm = Quantity::with_dim(0.5, Unit::Centimetre, -1);
    assert!(close(per_cm.shown(), 0.5 / 2.54), "shown {}", per_cm.shown());
}

#[test]
fn dimension_one_shows_as_before() {
    let q = Quantity::from_value(3.0, Unit::Centimetre);
    assert!(close(q.shown(), 3.0));
    assert!(close(Quantity::with_dim(q.stored, Unit::Centimetre, 1).shown(), 3.0));
}

#[test]
fn dimension_zero_is_a_plain_number() {
    // A final dimension of 0 is a plain number.
    let plain = Quantity::with_dim(6.0, Unit::Inch, 0);
    assert_eq!(plain.unit, Unit::Unitless);
    assert_eq!(plain.dim(), 0);
    assert!(close(plain.shown(), 6.0));
}

#[test]
fn an_area_unit_keeps_its_own_square() {
    // An acre already holds square inches: its dimension is 2 and the power is not applied again when it is shown.
    let acre = Quantity::from_value(1.0, Unit::Acre);
    assert!(close(acre.stored, 6_272_640.0));
    assert!(close(acre.shown(), 1.0));
    assert_eq!(acre.dim(), 2);
}

#[test]
fn value_in_and_convert_follow_the_dimension() {
    // A square value read in another length unit is that many of the squared unit.
    let area = Quantity::with_dim(1.0, Unit::Inch, 2);
    let in_cm = area.value_in(Unit::Centimetre).unwrap();
    assert!(close(in_cm, 6.4516), "{in_cm}");
    let converted = area.convert(Unit::Centimetre).unwrap();
    assert_eq!(converted.dim(), 2);
    assert!(close(converted.stored, 1.0));
    assert!(close(converted.shown(), 6.4516));
    // Another measure is still refused.
    assert!(area.value_in(Unit::Degree).is_none());
}

#[test]
fn from_shown_is_the_inverse_of_shown() {
    for (unit, dim) in [(Unit::Centimetre, 2), (Unit::Foot, 3), (Unit::Inch, -1), (Unit::Degree, 1), (Unit::Millimetre, 1)] {
        let q = Quantity::from_shown(6.4516, unit, dim);
        assert!(close(q.shown(), 6.4516), "{unit:?}^{dim}: {}", q.shown());
        assert_eq!(q.dim(), dim);
    }
    // 6.4516 square centimetres are one square inch.
    assert!(close(Quantity::from_shown(6.4516, Unit::Centimetre, 2).stored, 1.0));
    assert_eq!(Quantity::from_shown(3.0, Unit::Inch, 0).unit, Unit::Unitless);
}
