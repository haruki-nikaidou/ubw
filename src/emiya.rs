use tokio::io::{self, AsyncBufReadExt};

/// Waits for the user to input the complete UBW incantation in order.
/// Returns once the incantation is complete.
pub async fn wait_for_incantation() -> io::Result<()> {
    let incantation = [
        "I am the bone of my sword",
        "Steel is my body and fire is my blood",
        "I have created over a thousand blades",
        "Unknown to death",
        "Nor known to life",
        "Have withstood pain to create many weapons",
        "Yet those hands shall never hold anything",
        "So, as I pray, Unlimited Blade Works",
    ];

    println!("Please recite the UBW incantation:");

    let mut reader = io::BufReader::new(io::stdin());
    let mut line = String::new();
    let mut current_phrase = 0;

    while current_phrase < incantation.len() {
        line.clear();
        reader.read_line(&mut line).await?;

        if line.contains(incantation[current_phrase]) {
            current_phrase += 1;
            if current_phrase < incantation.len() {
                println!("Continue...");
            }
        }
    }

    println!("Incantation complete. Unlimited Blade Works activated!");
    Ok(())
}
