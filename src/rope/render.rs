use termimad::MadSkin;
use termimad::crossterm::style::{Color::*, Attribute::*};

fn make_rope_skin() -> MadSkin {
    let mut skin = MadSkin::default();
    
    // Customize headers
    skin.set_headers_fg(Cyan);
    for h in &mut skin.headers {
        h.add_attr(Bold);
    }
    
    // Bold / italic styling
    skin.bold.set_fg(Yellow);
    skin.italic.set_fg(DarkYellow);
    skin.italic.add_attr(Italic);
    
    // Inline code styling (green text)
    skin.inline_code.set_fg(Green);
    
    // Code block styling (sky blue / Ansi 111 foreground)
    skin.code_block.set_fg(AnsiValue(111));
    
    skin
}

#[allow(dead_code)]
pub fn print_markdown(md: &str) {
    let skin = make_rope_skin();
    skin.print_text(md);
}

pub fn print_help() {
    let help_content = include_str!("../../docs/Rope.md");
    print_markdown(help_content);
}
