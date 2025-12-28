//! Fun name generation for forks.
//!
//! Generates memorable names like "Gandalf the Deprecated" or
//! "Three Raccoons in a Trenchcoat, CPA".

use rand::prelude::IndexedRandom;

const FIRST_NAMES: &[&str] = &[
    // Simple/Classic
    "Gandalf", "Merlin", "Scott", "Trevor", "Kevin", "Barry", "Nigel",
    "Reginald", "Bartholomew", "Cornelius", "Thaddeus", "Mortimer",
    // Full names with titles
    "Mrs. Willoughby", "Father Paul Devonly", "Dr. Spaceman", "Sergeant Pickles",
    "Professor Snugglebottom", "Captain Whiskers", "Dame Judith", "Sir Reginald",
    "Bishop Flanagan", "Reverend Chaos", "Admiral Biscuits", "Colonel Mustard",
    // Cute/Pet names
    "Snookums", "Pudding", "Muffin", "Waffles", "Sprocket", "Gizmo", "Pebbles",
    "Mr. Fluffington", "Princess Thunderpaws", "Lord Wigglebottom", "Tiny Steve",
    // Dramatic/Dark
    "The Dread Lord Abaddon", "Xarthok the Defiler", "The Unnamed One",
    "Entropy Prime", "The Void Walker", "Chaos Incarnate", "The Final Arbiter",
    // Corporate/Modern
    "Chad from Marketing", "Brenda in HR", "The Scrum Master", "That Guy from IT",
    "Regional Manager Dwight", "Senior Vice President Jenkins", "Intern #47",
    // Absurd
    "A Swarm of Bees", "Three Raccoons in a Trenchcoat", "The Concept of Thursday",
    "The Stepmother You Never Wanted", "An Increasingly Nervous Flamingo", "Greg",
];

const SUFFIXES_SPACE: &[&str] = &[
    "the Magnificent", "the Terrible", "the Unready", "the Adequate",
    "the All-Knowing", "the Mostly-Knowing", "the Occasionally Correct",
    "the Destroyer of Worlds", "the Filer of Taxes", "the Sender of Emails",
    "the Inevitable", "the Procrastinator", "the Early-to-Bed",
    "the Devourer", "the Snack-Sized", "the Family-Sized",
    "the Recursive", "the Deprecated", "the Legacy Code",
];

const SUFFIXES_COMMA: &[&str] = &[
    "Attorney at Law", "CPA", "PhD", "Esq.", "MD",
    "Earl of Croix", "Duke of URL", "Baron of the Spreadsheet",
    "Viscount of the Third Floor", "Lord of the Ping", "Count of Monte Crisco",
    "Regional Manager", "Associate Vice President", "Junior Senior Developer",
    "Defender of the Realm", "Keeper of the Sacred Changelog",
    "who is running late", "who forgot to mute", "who meant to reply-all",
    "who's not angry, just disappointed",
];

const SUFFIXES_DASH: &[&str] = &[
    "Sexiest Person, 1998-99 (Elevator World Magazine)",
    "Winner, Most Consistent (Participation Magazine)",
    "As Seen on TV's Matlock",
    "Now With 20% More Existential Dread!",
    "Terms and Conditions Apply",
    "Voted 'Most Likely to Defecate Standing'",
    "Certified Pre-Owned", "Some Assembly Required",
    "Batteries Not Included", "Your Mileage May Vary",
    "Not Valid in Quebec", "Please Consult Your Doctor",
    "Your Childhood Imaginary Friend", "Who's not my real mum",
];

const SUFFIXES_OF_THE: &[&str] = &[
    "Flesh Cathedral", "Screaming Void", "Infinite Spreadsheet",
    "Forbidden Repository", "Haunted Codebase", "Eternal Standup",
    "Third-Floor Breakroom", "Unclosed Parenthesis", "Merge Conflict",
    "Sacred Timeline", "Forbidden Snack Drawer", "Lost Documentation",
    "Thousand Jira Tickets", "Unanswered Slack Messages", "Pending PRs",
];

/// A generated name with short and full versions.
#[derive(Debug, Clone)]
pub struct ForkName {
    /// Short name (just the first part).
    pub nickname: String,
    /// Complete generated name.
    pub full_name: String,
}

/// Generate a random fork name.
pub fn generate_name() -> ForkName {
    let mut rng = rand::rng();

    let first = FIRST_NAMES.choose(&mut rng).unwrap_or(&"Fork");

    // Pick a random suffix category
    let suffix_type = rand::random::<u8>() % 4;
    let (separator, suffixes) = match suffix_type {
        0 => (" ", SUFFIXES_SPACE),
        1 => (", ", SUFFIXES_COMMA),
        2 => ("—", SUFFIXES_DASH),
        _ => (" of the ", SUFFIXES_OF_THE),
    };

    let suffix = suffixes.choose(&mut rng).unwrap_or(&"the Unknown");

    ForkName {
        nickname: first.to_string(),
        full_name: format!("{}{}{}", first, separator, suffix),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_name() {
        let name = generate_name();
        assert!(!name.nickname.is_empty());
        assert!(!name.full_name.is_empty());
        assert!(name.full_name.contains(&name.nickname));
    }
}
