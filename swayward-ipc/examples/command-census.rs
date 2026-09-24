use std::io::{self, BufRead as _};

fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.expect("failed to read command census");
        let (name, probe) = line
            .split_once('\t')
            .expect("expected a tab-separated command and probe");
        let accepted = swayward_ipc::command::parse(probe)
            .into_iter()
            .all(|result| result.is_ok());
        println!("{name}\t{}", if accepted { "accept" } else { "reject" });
    }
}
