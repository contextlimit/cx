use super::*;

fn parse_cli<const N: usize>(args: [&'static str; N]) -> Cli {
    parse_from_cx_args(args)
}

mod auto_routing;
mod command_shapes;
