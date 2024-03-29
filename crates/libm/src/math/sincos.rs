/* origin: FreeBSD /usr/src/lib/msun/src/s_sin.c */
/*
 * ====================================================
 * Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
 *
 * Developed at SunPro, a Sun Microsystems, Inc. business.
 * Permission to use, copy, modify, and distribute this
 * software is freely granted, provided that this notice
 * is preserved.
 * ====================================================
 */

use super::{get_high_word, k_cos, k_sin, rem_pio2};

#[inline]
#[cfg_attr(all(test, assert_no_panic), no_panic::no_panic)]
pub extern "C" fn sincos(x: f64) -> (f64, f64) {
    let s: f64;
    let c: f64;
    let mut ix: u32;

    ix = get_high_word(x);
    ix &= 0x7fffffff;

    /* |x| ~< pi/4 */
    if ix <= 0x3fe921fb {
        /* if |x| < 2**-27 * sqrt(2) */
        if ix < 0x3e46a09e {
            /* raise inexact if x!=0 and underflow if subnormal */
            let x1p120 = f64::from_bits(0x4770000000000000); // 0x1p120 == 2^120
            if ix < 0x00100000 {
                force_eval!(x / x1p120);
            } else {
                force_eval!(x + x1p120);
            }
            return (x, 1.0);
        }
        return (k_sin(x, 0.0, 0), k_cos(x, 0.0));
    }

    /* sincos(Inf or NaN) is NaN */
    if ix >= 0x7ff00000 {
        let rv = x - x;
        return (rv, rv);
    }

    /* argument reduction needed */
    let (n, y0, y1) = rem_pio2(x);
    s = k_sin(y0, y1, 1);
    c = k_cos(y0, y1);
    match n & 3 {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        3 => (-c, s),
        #[cfg(feature = "checked")]
        _ => unreachable!(),
        #[cfg(not(feature = "checked"))]
        _ => (0.0, 1.0),
    }
}
