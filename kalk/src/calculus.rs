use crate::as_number_or_zero;
use crate::ast;
use crate::ast::Expr;
use crate::ast::Identifier;
use crate::ast::Stmt;
use crate::float;
use crate::interpreter;
use crate::kalk_value::KalkValue;
use crate::lexer::TokenKind;
use crate::parser::CalcError;

pub fn derive_func(
    context: &mut interpreter::Context,
    name: &Identifier,
    argument: KalkValue,
) -> Result<KalkValue, CalcError> {
    const H: f64 = 0.000001;

    let unit = argument.get_unit().cloned();
    let argument_with_h = ast::build_literal_ast(&argument.clone().add_without_unit(&H.into()));
    let argument_without_h = ast::build_literal_ast(&argument.sub_without_unit(&H.into()));
    let new_identifier = Identifier::from_name_and_primes(&name.pure_name, name.prime_count - 1);

    let f_x_h = interpreter::eval_fn_call_expr(
        context,
        &new_identifier,
        &[argument_with_h],
        unit.as_ref(),
    )?;
    let f_x = interpreter::eval_fn_call_expr(
        context,
        &new_identifier,
        &[argument_without_h],
        unit.as_ref(),
    )?;

    Ok(f_x_h
        .sub_without_unit(&f_x)
        .div_without_unit(&(2f64 * H).into())
        .round_if_needed())
}

pub fn integrate_with_unknown_variable(
    context: &mut interpreter::Context,
    a: &Expr,
    b: &Expr,
    expr: &Expr,
) -> Result<KalkValue, CalcError> {
    let mut integration_variable: Option<&str> = None;

    // integral(a, b, expr dx)
    if let Expr::Binary(_, TokenKind::Star, right) = expr {
        if let Expr::Var(right_name) = &**right {
            if right_name.full_name.starts_with('d') {
                // Take the value, but remove the d, so that only eg. x is left from dx
                integration_variable = Some(&right_name.full_name[1..]);
            }
        }
    }

    if integration_variable.is_none() {
        return Err(CalcError::ExpectedDx);
    }

    // "dx" is still in the expression. Set dx = 1, so that it doesn't affect the expression value.
    context.symbol_table.set(Stmt::VarDecl(
        Identifier::from_full_name(&format!("d{}", integration_variable.unwrap())),
        Box::new(Expr::Literal(1f64)),
    ));

    Ok(integrate(context, a, b, expr, integration_variable.unwrap())?.round_if_needed())
}

pub fn integrate(
    context: &mut interpreter::Context,
    a: &Expr,
    b: &Expr,
    expr: &Expr,
    integration_variable: &str,
) -> Result<KalkValue, CalcError> {
    Ok(simpsons_rule(context, a, b, expr, integration_variable)?.round_if_needed())
}

/// Composite Simpson's 3/8 rule
fn simpsons_rule(
    context: &mut interpreter::Context,
    a_expr: &Expr,
    b_expr: &Expr,
    expr: &Expr,
    integration_variable: &str,
) -> Result<KalkValue, CalcError> {
    let mut result_real = float!(0);
    let mut result_imaginary = float!(0);
    let original_variable_value = context
        .symbol_table
        .get_and_remove_var(integration_variable);

    const N: i32 = 900;
    let a = interpreter::eval_expr(context, a_expr, None)?;
    let b = interpreter::eval_expr(context, b_expr, None)?;
    let h = (b.sub_without_unit(&a)).div_without_unit(&KalkValue::from(N));
    for i in 0..=N {
        let variable_value = a
            .clone()
            .add_without_unit(&KalkValue::from(i).mul_without_unit(&h.clone()));
        context.symbol_table.set(Stmt::VarDecl(
            Identifier::from_full_name(integration_variable),
            Box::new(crate::ast::build_literal_ast(&variable_value)),
        ));

        let factor = KalkValue::from(match i {
            0 | N => 1,
            _ if i % 3 == 0 => 2,
            _ => 3,
        } as f64);

        // factor * f(x_n)
        let (mul_real, mul_imaginary, _) = as_number_or_zero!(
            factor.mul_without_unit(&interpreter::eval_expr(context, expr, None)?)
        );
        result_real += mul_real;
        result_imaginary += mul_imaginary;
    }

    if let Some(value) = original_variable_value {
        context.symbol_table.insert(value);
    } else {
        context
            .symbol_table
            .get_and_remove_var(integration_variable);
    }

    let result = KalkValue::Number(result_real, result_imaginary, None);
    let (h_real, h_imaginary, h_unit) = as_number_or_zero!(h);

    Ok(result.mul_without_unit(&KalkValue::Number(
        3f64 / 8f64 * h_real,
        3f64 / 8f64 * h_imaginary,
        h_unit,
    )))
}

#[cfg(test)]
mod tests {
    use crate::ast;
    use crate::calculus::Identifier;
    use crate::calculus::Stmt;
    use crate::float;
    use crate::interpreter;
    use crate::kalk_value::KalkValue;
    use crate::lexer::TokenKind::*;
    use crate::symbol_table::SymbolTable;
    use crate::test_helpers::*;

    fn get_context(symbol_table: &mut SymbolTable) -> interpreter::Context {
        interpreter::Context::new(
            symbol_table,
            "",
            #[cfg(feature = "rug")]
            63u32,
            None,
        )
    }

    #[test]
    fn test_derive_func() {
        let mut symbol_table = SymbolTable::new();
        let mut context = get_context(&mut symbol_table);
        context.symbol_table.insert(Stmt::FnDecl(
            Identifier::from_full_name("f"),
            vec![String::from("x")],
            binary(
                literal(2.5f64),
                Star,
                binary(var("x"), Power, literal(3f64)),
            ),
        ));

        let call = Stmt::Expr(fn_call("f'", vec![*literal(12.3456f64)]));
        assert!(cmp(
            context.interpret(vec![call]).unwrap().unwrap().to_f64(),
            1143.10379f64
        ));
    }

    #[test]
    fn test_derive_complex_func() {
        let mut symbol_table = SymbolTable::new();
        let mut context = get_context(&mut symbol_table);
        context.symbol_table.insert(Stmt::FnDecl(
            Identifier::from_full_name("f"),
            vec![String::from("x")],
            binary(
                binary(
                    literal(1.5f64),
                    Star,
                    binary(var("x"), Power, literal(2f64)),
                ),
                Plus,
                binary(binary(var("x"), Power, literal(2f64)), Star, var("i")),
            ),
        ));

        let call = Stmt::Expr(fn_call("f'", vec![*var("e")]));
        let result = context.interpret(vec![call]).unwrap().unwrap();
        assert!(cmp(result.to_f64(), 8.15484f64));
        assert!(cmp(result.imaginary_to_f64(), 5.43656));
    }

    #[test]
    fn test_derive_func_with_complex_argument() {
        let mut symbol_table = SymbolTable::new();
        let mut context = get_context(&mut symbol_table);
        context.symbol_table.insert(Stmt::FnDecl(
            Identifier::from_full_name("f"),
            vec![String::from("x")],
            binary(
                binary(literal(3f64), Star, var("x")),
                Plus,
                binary(
                    literal(0.5f64),
                    Star,
                    binary(var("x"), Power, literal(3f64)),
                ),
            ),
        ));

        let result = super::derive_func(
            &mut context,
            &Identifier::from_full_name("f'"),
            KalkValue::Number(float!(2f64), float!(3f64), None),
        )
        .unwrap();
        assert!(cmp(result.to_f64(), -4.5f64) || cmp(result.to_f64(), -4.499999f64));
        assert!(cmp(result.imaginary_to_f64(), 18f64));
    }

    #[test]
    fn test_integrate_with_unknown_variable() {
        let mut symbol_table = SymbolTable::new();
        let mut context = get_context(&mut symbol_table);
        let result = super::integrate_with_unknown_variable(
            &mut context,
            &*literal(2f64),
            &*literal(4f64),
            &*binary(var("x"), Star, var("dx")),
        )
        .unwrap();

        assert!(cmp(result.to_f64(), 6f64));
    }

    #[test]
    fn test_integrate() {
        let mut symbol_table = SymbolTable::new();
        let mut context = get_context(&mut symbol_table);
        let result = super::integrate(
            &mut context,
            &*literal(2f64),
            &*literal(4f64),
            &*var("x"),
            "x",
        )
        .unwrap();

        assert!(cmp(result.to_f64(), 6f64));
    }

    #[test]
    fn test_integrate_complex() {
        let mut symbol_table = SymbolTable::new();
        let mut context = get_context(&mut symbol_table);
        let result = super::integrate(
            &mut context,
            &*literal(2f64),
            &ast::build_literal_ast(&KalkValue::Number(float!(3f64), float!(4f64), None)),
            &*binary(var("x"), Star, var("i")),
            "x",
        )
        .unwrap();

        assert!(cmp(result.to_f64(), -12f64));
        assert!(cmp(result.imaginary_to_f64(), -5.5f64));
    }
}
