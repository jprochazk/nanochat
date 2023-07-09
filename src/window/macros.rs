macro_rules! exit_if {
  ($e:expr, $c:ident) => {
    match $e {
      Ok(v) => v,
      Err(e) => {
        eprintln!("{e}");
        *$c = ControlFlow::ExitWithCode(1);
        return;
      }
    }
  };
}
