use josh_filter::eggopt::{egg_optimize, equivalent};
use josh_filter::flang::parse::parse;

#[test]
fn real_filters_stay_equivalent() {
    for spec in [
        ":/a",
        ":/a:/b",
        ":[x=:/a:/b:/d,y=:/a:/c:/d]",
        ":subtract[a=:[::x/,::y/,::z/],b=:[::x/,::y/]]",
        ":subtract[a=:[::x/,::y/,::z/],b=:[::x/,::y/,::w/]]",
    ] {
        let f = parse(spec).expect(spec);
        assert!(
            equivalent(f, egg_optimize(f)),
            "egg_optimize broke equivalence for {spec:?}"
        );
    }
}
