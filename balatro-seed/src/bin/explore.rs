fn main() {
    let mut args = std::env::args().skip(1);
    let mut seed: Option<String> = None;
    let mut opts = balatro_seed::render::RenderOptions::default();

    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--ante" => {
                i += 1;
                opts.max_ante = argv[i].parse().expect("--ante expects an integer");
            }
            "--cards-per-ante" => {
                i += 1;
                opts.cards_per_ante = argv[i]
                    .split(',')
                    .map(|n| n.trim().parse().expect("cards-per-ante expects integers"))
                    .collect();
            }
            "--vouchers" => {
                i += 1;
                opts.vouchers_bought = match argv[i].as_str() {
                    "bought" => true,
                    "offered" => false,
                    other => panic!("--vouchers expects bought|offered, got {other}"),
                };
            }
            "--no-activate-vouchers" => opts.activate_vouchers = false,
            "--fresh-profile" => opts.fresh_profile = true,
            "--ante-0" => opts.ante_0 = true,
            other if seed.is_none() => seed = Some(other.to_string()),
            other => panic!("unrecognized argument: {other}"),
        }
        i += 1;
    }
    let _ = &mut args;

    let seed = seed.unwrap_or_else(|| {
        eprintln!(
            "usage: explore SEED [--ante N] [--cards-per-ante 15,50,...] \
             [--vouchers bought|offered] [--no-activate-vouchers] \
             [--fresh-profile] [--ante-0]"
        );
        std::process::exit(1);
    });

    print!("{}", balatro_seed::render::render(&seed, &opts));
}
