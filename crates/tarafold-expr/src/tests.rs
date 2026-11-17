use tarafold_core::Rng;

use super::*;

fn names(n: &[&str]) -> Vec<String> {
    n.iter().map(|s| s.to_string()).collect()
}

#[test]
fn eval_and_protected_ops() {
    let n = names(&["x", "y"]);
    let e = parse("2*x + y^2 - log(x)", &n).unwrap();
    let v = e.eval_point(&[1.0, 3.0]);
    assert!((v - 11.0).abs() < 1e-12);
    assert!(
        parse("log(x)", &n)
            .unwrap()
            .eval_point(&[-1.0, 0.0])
            .is_nan()
    );
    assert!(parse("x / y", &n).unwrap().eval_point(&[1.0, 0.0]).is_nan());
    assert!(
        parse("x ^ 0.5", &n)
            .unwrap()
            .eval_point(&[-4.0, 0.0])
            .is_nan()
    );
    assert_eq!(parse("x ^ 2", &n).unwrap().eval_point(&[-3.0, 0.0]), 9.0);
    let cols = e.eval_columns(&[&[1.0, 2.0], &[3.0, 1.0]], 2).unwrap();
    assert!((cols[0] - 11.0).abs() < 1e-12);
    assert!((cols[1] - (5.0 - 2f64.ln())).abs() < 1e-12);
}

#[test]
fn precedence_and_associativity() {
    let n = names(&["a", "b", "c"]);
    let at = [2.0, 3.0, 4.0];
    let check = |s: &str, want: f64| {
        let v = parse(s, &n).unwrap().eval_point(&at);
        assert!((v - want).abs() < 1e-9, "{s} = {v}, want {want}");
    };
    check("a - b - c", -5.0);
    check("a / b / c", 2.0 / 12.0);
    check("a ^ b ^ 2", 512.0);
    check("-a ^ 2", -4.0);
    check("(-a) ^ 2", 4.0);
    check("a * -b", -6.0);
    check("2e-1 * c", 0.8);
}

#[test]
fn print_examples() {
    let n = names(&["orders", "price", "Marketing Spend"]);
    let e = parse("orders * price", &n).unwrap();
    assert_eq!(e.to_infix(&n), "orders * price");
    let e = parse("2.5 * `Marketing Spend` ^ 0.7 - (price - 1)", &n).unwrap();
    assert_eq!(
        e.to_infix_pretty(&n, 4),
        "2.5 * `Marketing Spend`^0.7 - (price - 1)"
    );
    assert!(e.to_latex(&n, 4).contains("\\mathrm{Marketing\\ Spend}"));
}

fn random_expr(r: &mut Rng, depth: usize, n_vars: u16) -> Expr {
    if depth == 0 || r.uniform() < 0.25 {
        return if r.uniform() < 0.5 {
            Expr::var(r.below(n_vars as usize) as u16)
        } else {
            let c = (r.normal() * 100.0).round() / 10.0;
            Expr::constant(if r.uniform() < 0.1 { c * 1e-9 } else { c })
        };
    }
    if r.uniform() < 0.3 {
        let ops = [
            UnOp::Neg,
            UnOp::Exp,
            UnOp::Log,
            UnOp::Sqrt,
            UnOp::Square,
            UnOp::Abs,
            UnOp::Inv,
        ];
        let a = random_expr(r, depth - 1, n_vars);
        // Parser folds `-const` into a negative constant; mirror that.
        let op = ops[r.below(ops.len())];
        if op == UnOp::Neg && matches!(a.nodes(), [Node::Const(_)]) {
            return a;
        }
        return a.unary(op);
    }
    let ops = [BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::Div, BinOp::Pow];
    let a = random_expr(r, depth - 1, n_vars);
    let b = random_expr(r, depth - 1, n_vars);
    Expr::binary(ops[r.below(ops.len())], a, b)
}

#[test]
fn parse_print_roundtrip_random() {
    let n = names(&["x", "y", "z"]);
    let mut r = Rng::derive(1, &[]);
    for _ in 0..3000 {
        let e = random_expr(&mut r, 5, 3);
        let s = e.to_infix(&n);
        let back = parse(&s, &n).unwrap_or_else(|err| panic!("{s}: {err}"));
        assert_eq!(back, e, "{s}");
    }
}

#[test]
fn derivatives_match_finite_differences() {
    let n = names(&["x", "y"]);
    let mut r = Rng::derive(2, &[]);
    let mut checked = 0;
    for _ in 0..2000 {
        let e = random_expr(&mut r, 4, 2);
        let x = [r.range(0.5, 3.0), r.range(0.5, 3.0)];
        for var in 0..2u16 {
            let (v, d) = e.eval_partial(&x, var);
            let h = 1e-6 * x[var as usize].abs().max(1.0);
            let mut xp = x;
            let mut xm = x;
            xp[var as usize] += h;
            xm[var as usize] -= h;
            let fd = (e.eval_point(&xp) - e.eval_point(&xm)) / (2.0 * h);
            if !(v.is_finite() && d.is_finite() && fd.is_finite()) || v.abs() > 1e6 || d.abs() > 1e6
            {
                continue;
            }
            assert!(
                (d - fd).abs() <= 1e-4 * (1.0 + d.abs()),
                "{} d/d{var} at {x:?}: ad={d} fd={fd}",
                e.to_infix(&n)
            );
            checked += 1;
        }
    }
    assert!(checked > 1000, "only {checked} finite cases");
}

#[test]
fn simplify_preserves_values() {
    let n = names(&["x"]);
    let e = parse("(2 * 3) * x * 1 + 0 + x ^ 1", &n).unwrap();
    let s = e.simplify();
    assert_eq!(s.to_infix(&n), "6.0 * x + x");
    assert_eq!(s.eval_point(&[2.0]), e.eval_point(&[2.0]));
}

#[test]
fn rejects_bad_input() {
    let n = names(&["x"]);
    assert!(parse("x +", &n).is_err());
    assert!(parse("foo(x)", &n).is_err());
    assert!(parse("y", &n).is_err());
    assert!(parse("x; rm -rf", &n).is_err());
    assert!(parse(&"(".repeat(1000), &n).is_err());
}
