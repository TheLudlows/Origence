fn main() {
    let jieba = jieba_rs::Jieba::new();
    for text in std::env::args().skip(1) {
        let terms: Vec<String> = jieba
            .cut_for_search(&text, false)
            .into_iter()
            .filter(|term| term.chars().any(char::is_alphanumeric))
            .map(str::to_lowercase)
            .collect();
        println!("{}\t{}", text, terms.join(" "));
    }
}
