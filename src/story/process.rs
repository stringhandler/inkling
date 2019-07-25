//! Process lines to their final form, which will be displayed to the user.

use crate::{
    error::InklingError,
    follow::{ChoiceExtra, LineDataBuffer},
    line::{Condition, Content, InternalLine},
};

use super::{
    address::Address,
    story::{get_stitch, Choice, Knots, Line, LineBuffer},
};

/// Process full `LineData` lines to their final state: remove empty lines, add newlines
/// unless glue is present.
pub fn process_buffer(into_buffer: &mut LineBuffer, from_buffer: LineDataBuffer) {
    let mut iter = from_buffer
        .into_iter()
        .filter(|line| !line.text().trim().is_empty())
        .peekable();

    while let Some(mut line) = iter.next() {
        add_line_ending(&mut line, iter.peek());

        into_buffer.push(Line {
            text: line.text(),
            tags: line.tags,
        });
    }
}

/// Prepared the choices with the text that will be displayed to the user.
/// Preserve line tags in case processing is desired. Choices are filtered
/// based on a set condition (currently: visited or not, unless sticky).
pub fn prepare_choices_for_user(
    choices: &[ChoiceExtra],
    current_address: &Address,
    knots: &Knots,
) -> Result<Vec<Choice>, InklingError> {
    get_available_choices(choices, current_address, knots, false)
}

/// Prepare a list of fallback choices from the given set. The first choice will be
/// automatically selected.
pub fn get_fallback_choices(
    choices: &[ChoiceExtra],
    current_address: &Address,
    knots: &Knots,
) -> Result<Vec<Choice>, InklingError> {
    get_available_choices(choices, current_address, knots, true)
}

fn get_available_choices(
    choices: &[ChoiceExtra],
    current_address: &Address,
    knots: &Knots,
    fallback: bool,
) -> Result<Vec<Choice>, InklingError> {
    let choices_with_filter_values =
        zip_choices_with_filter_values(choices, current_address, knots, fallback)?;

    let filtered_choices = choices_with_filter_values
        .into_iter()
        .filter_map(|(keep, choice)| if keep { Some(choice) } else { None })
        .collect();

    Ok(filtered_choices)
}

fn zip_choices_with_filter_values(
    choices: &[ChoiceExtra],
    current_address: &Address,
    knots: &Knots,
    fallback: bool,
) -> Result<Vec<(bool, Choice)>, InklingError> {
    let checked_choices = check_choices_for_conditions(choices, current_address, knots, fallback)?;

    let filtered_choices = choices
        .iter()
        .enumerate()
        .map(|(i, ChoiceExtra { choice_data, .. })| Choice {
            text: choice_data.selection_text.text().trim().to_string(),
            tags: choice_data.selection_text.tags.clone(),
            index: i,
        })
        .zip(checked_choices.into_iter())
        .map(|(choice, keep)| (keep, choice))
        .collect();

    Ok(filtered_choices)
}

fn check_choices_for_conditions(
    choices: &[ChoiceExtra],
    current_address: &Address,
    knots: &Knots,
    keep_only_fallback: bool,
) -> Result<Vec<bool>, InklingError> {
    let mut checked_conditions = Vec::new();

    for ChoiceExtra {
        num_visited,
        choice_data,
    } in choices.iter()
    {
        let mut keep = true;

        for condition in choice_data.conditions.iter() {
            keep = check_condition(condition, current_address, knots)?;

            if !keep {
                break;
            }
        }

        keep = keep
            && (choice_data.is_sticky || *num_visited == 0)
            && (choice_data.is_fallback == keep_only_fallback);

        checked_conditions.push(keep);
    }

    Ok(checked_conditions)
}

/// Add a newline character if the line is not glued to the next. Retain only a single
/// whitespace between the lines if they are glued.
fn add_line_ending(line: &mut InternalLine, next_line: Option<&InternalLine>) {
    let glue = next_line
        .map(|next_line| line.glue_end || next_line.glue_begin)
        .unwrap_or(false);

    let whitespace = glue && {
        next_line
            .map(|next_line| line.text().ends_with(' ') || next_line.text().starts_with(' '))
            .unwrap_or(false)
    };

    if !glue || whitespace {
        let mut text = line.text().trim().to_string();

        if whitespace {
            text.push(' ');
        }

        if !glue {
            text.push('\n');
        }

        match line.chunk.items[0] {
            Content::Text(ref mut content) => *content = text,
            _ => unreachable!(),
        }
    }
}

fn check_condition(
    condition: &Condition,
    current_address: &Address,
    knots: &Knots,
) -> Result<bool, InklingError> {
    match condition {
        Condition::NumVisits {
            name,
            rhs_value,
            ordering,
            not,
        } => {
            let address = Address::from_target_address(name, current_address, knots)?;
            let num_visits = get_stitch(&address, knots)?.num_visited as i32;

            let value = num_visits.cmp(rhs_value) == *ordering;

            if *not {
                Ok(!value)
            } else {
                Ok(value)
            }
        }
    }
}

/// If the story was followed with an invalid choice we want to collect as much information
/// about it as possible. This is done when first encountering the error as the stack
/// is followed, which fills in which `ChoiceData` values were available and which index
/// was used to select with.
///
/// This function fills in the rest of the stub.
pub fn fill_in_invalid_error(
    error_stub: InklingError,
    made_choice: &Choice,
    current_address: &Address,
    knots: &Knots,
) -> InklingError {
    match error_stub {
        InklingError::InvalidChoice {
            index,
            internal_choices,
            ..
        } => {
            let presented_choices =
                zip_choices_with_filter_values(&internal_choices, current_address, knots, false)
                    .unwrap_or(Vec::new());

            InklingError::InvalidChoice {
                index,
                choice: Some(made_choice.clone()),
                internal_choices,
                presented_choices,
            }
        }
        _ => error_stub,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        consts::ROOT_KNOT_NAME,
        knot::{Knot, Stitch},
        line::{InternalChoice, InternalChoiceBuilder, InternalLineBuilder},
    };

    use std::{cmp::Ordering, collections::HashMap, str::FromStr};

    fn get_mock_address_and_knots() -> (Address, Knots) {
        let empty_hash_map = HashMap::new();
        let empty_address = Address {
            knot: "".to_string(),
            stitch: "".to_string(),
        };

        (empty_address, empty_hash_map)
    }

    fn create_choice_extra(num_visited: u32, choice_data: InternalChoice) -> ChoiceExtra {
        ChoiceExtra {
            num_visited,
            choice_data,
        }
    }

    #[test]
    fn check_some_conditions_against_number_of_visits_in_a_hash_map() {
        let name = "knot_name".to_string();

        let mut stitch = Stitch::from_str("").unwrap();
        stitch.num_visited = 3;

        let mut stitches = HashMap::new();
        stitches.insert(ROOT_KNOT_NAME.to_string(), stitch);

        let mut knots = HashMap::new();
        knots.insert(
            name.clone(),
            Knot {
                default_stitch: ROOT_KNOT_NAME.to_string(),
                stitches,
            },
        );

        let current_address = Address::from_root_knot("knot_name", &knots).unwrap();

        let greater_than_condition = Condition::NumVisits {
            name: name.clone(),
            rhs_value: 2,
            ordering: Ordering::Greater,
            not: false,
        };

        assert!(check_condition(&greater_than_condition, &current_address, &knots).unwrap());

        let less_than_condition = Condition::NumVisits {
            name: name.clone(),
            rhs_value: 2,
            ordering: Ordering::Less,
            not: false,
        };

        assert!(!check_condition(&less_than_condition, &current_address, &knots).unwrap());

        let equal_condition = Condition::NumVisits {
            name: name.clone(),
            rhs_value: 3,
            ordering: Ordering::Equal,
            not: false,
        };

        assert!(check_condition(&equal_condition, &current_address, &knots).unwrap());

        let not_equal_condition = Condition::NumVisits {
            name: name.clone(),
            rhs_value: 3,
            ordering: Ordering::Equal,
            not: true,
        };

        assert!(!check_condition(&not_equal_condition, &current_address, &knots).unwrap());
    }

    #[test]
    fn if_condition_checks_knot_that_is_not_in_map_an_error_is_raised() {
        let knots = HashMap::new();

        let gt_condition = Condition::NumVisits {
            name: "knot_name".to_string(),
            rhs_value: 0,
            ordering: Ordering::Greater,
            not: false,
        };

        let current_address = Address {
            knot: "".to_string(),
            stitch: "".to_string(),
        };

        assert!(check_condition(&gt_condition, &current_address, &knots).is_err());
    }

    #[test]
    fn processing_line_buffer_removes_empty_lines() {
        let text = "Mr. and Mrs. Doubtfire";

        let buffer = vec![
            InternalLineBuilder::from_string(text).build(),
            InternalLineBuilder::from_string("").build(),
            InternalLineBuilder::from_string(text).build(),
        ];

        let mut processed = Vec::new();
        process_buffer(&mut processed, buffer);

        assert_eq!(processed.len(), 2);
        assert_eq!(processed[0].text.trim(), text);
        assert_eq!(processed[1].text.trim(), text);
    }

    #[test]
    fn processing_line_buffer_trims_extra_whitespace() {
        let buffer = vec![
            InternalLineBuilder::from_string("    Hello, World!    ").build(),
            InternalLineBuilder::from_string("    Hello right back at you!  ").build(),
        ];

        let mut processed = Vec::new();
        process_buffer(&mut processed, buffer);

        assert_eq!(processed.len(), 2);
        assert_eq!(processed[0].text.trim(), "Hello, World!");
        assert_eq!(processed[1].text.trim(), "Hello right back at you!");
    }

    #[test]
    fn processing_line_buffer_adds_newlines_if_no_glue() {
        let text = "Mr. and Mrs. Doubtfire";

        let buffer = vec![
            InternalLineBuilder::from_string(text).build(),
            InternalLineBuilder::from_string(text).build(),
        ];

        let mut processed = Vec::new();
        process_buffer(&mut processed, buffer);

        assert!(processed[0].text.ends_with('\n'));
        assert!(processed[1].text.ends_with('\n'));
    }

    #[test]
    fn processing_line_buffer_removes_newlines_between_lines_with_glue_end_on_first() {
        let text = "Mr. and Mrs. Doubtfire";

        let buffer = vec![
            InternalLineBuilder::from_string(text)
                .with_glue_end()
                .build(),
            InternalLineBuilder::from_string(text).build(),
        ];

        let mut processed = Vec::new();
        process_buffer(&mut processed, buffer);

        assert!(!processed[0].text.ends_with('\n'));
        assert!(processed[1].text.ends_with('\n'));
    }

    #[test]
    fn processing_line_buffer_removes_newlines_between_lines_with_glue_start_on_second() {
        let text = "Mr. and Mrs. Doubtfire";

        let buffer = vec![
            InternalLineBuilder::from_string(text).build(),
            InternalLineBuilder::from_string(text)
                .with_glue_begin()
                .build(),
        ];

        let mut processed = Vec::new();
        process_buffer(&mut processed, buffer);

        assert!(!processed[0].text.ends_with('\n'));
        assert!(processed[1].text.ends_with('\n'));
    }

    #[test]
    fn processing_line_buffer_with_glue_works_across_empty_lines() {
        let text = "Mr. and Mrs. Doubtfire";

        let buffer = vec![
            InternalLineBuilder::from_string(text).build(),
            InternalLineBuilder::from_string("").build(),
            InternalLineBuilder::from_string(text)
                .with_glue_begin()
                .build(),
        ];

        let mut processed = Vec::new();
        process_buffer(&mut processed, buffer);

        assert!(!processed[0].text.ends_with('\n'));
        assert!(processed[1].text.ends_with('\n'));
    }

    #[test]
    fn processing_line_buffer_sets_newline_on_last_line_regardless_of_glue() {
        let line = InternalLineBuilder::from_string("Mr. and Mrs. Doubtfire")
            .with_glue_end()
            .build();

        let buffer = vec![line];

        let mut processed = Vec::new();
        process_buffer(&mut processed, buffer);

        assert!(processed[0].text.ends_with('\n'));
    }

    #[test]
    fn processing_line_buffer_keeps_single_whitespace_between_lines_with_glue() {
        let line1 = InternalLineBuilder::from_string("Ends with whitespace before glue, ")
            .with_glue_end()
            .build();
        let line2 = InternalLineBuilder::from_string(" starts with whitespace after glue")
            .with_glue_begin()
            .build();

        let buffer = vec![line1, line2];

        let mut processed = Vec::new();
        process_buffer(&mut processed, buffer);

        assert!(processed[0].text.ends_with(' '));
        assert!(!processed[1].text.starts_with(' '));
    }

    #[test]
    fn processing_line_buffer_preserves_tags() {
        let text = "Mr. and Mrs. Doubtfire";
        let tags = vec!["tag 1".to_string(), "tag 2".to_string()];

        let line = InternalLineBuilder::from_string(text)
            .with_tags(&tags)
            .build();

        let buffer = vec![line];

        let mut processed = Vec::new();
        process_buffer(&mut processed, buffer);

        assert_eq!(processed[0].tags, tags);
    }

    #[test]
    fn preparing_choices_returns_selection_text_lines() {
        let choice1 = InternalChoiceBuilder::from_selection_string("Choice 1").build();

        let choice2 = InternalChoiceBuilder::from_selection_string("Choice 2").build();

        let choices = vec![
            create_choice_extra(0, choice1),
            create_choice_extra(0, choice2),
        ];

        let (empty_address, empty_hash_map) = get_mock_address_and_knots();
        let displayed_choices =
            prepare_choices_for_user(&choices, &empty_address, &empty_hash_map).unwrap();

        assert_eq!(displayed_choices.len(), 2);
        assert_eq!(&displayed_choices[0].text, "Choice 1");
        assert_eq!(&displayed_choices[1].text, "Choice 2");
    }

    #[test]
    fn preparing_choices_preserves_tags() {
        let tags = vec!["tag 1".to_string(), "tag 2".to_string()];
        let choice = InternalChoiceBuilder::from_string("Choice with tags")
            .with_tags(&tags)
            .build();

        let choices = vec![create_choice_extra(0, choice)];

        let (empty_address, empty_hash_map) = get_mock_address_and_knots();
        let displayed_choices =
            prepare_choices_for_user(&choices, &empty_address, &empty_hash_map).unwrap();

        assert_eq!(displayed_choices[0].tags, tags);
    }

    #[test]
    fn processing_choices_checks_conditions() {
        let name = "knot_name".to_string();

        let mut stitch = Stitch::from_str("").unwrap();
        stitch.num_visited = 1;

        let mut stitches = HashMap::new();
        stitches.insert(ROOT_KNOT_NAME.to_string(), stitch);

        let mut knots = HashMap::new();
        knots.insert(
            name.clone(),
            Knot {
                default_stitch: ROOT_KNOT_NAME.to_string(),
                stitches,
            },
        );

        let current_address = Address::from_root_knot("knot_name", &knots).unwrap();

        let fulfilled_condition = Condition::NumVisits {
            name: name.clone(),
            rhs_value: 0,
            ordering: Ordering::Greater,
            not: false,
        };

        let unfulfilled_condition = Condition::NumVisits {
            name: name.clone(),
            rhs_value: 2,
            ordering: Ordering::Greater,
            not: false,
        };

        let choice1 = InternalChoiceBuilder::from_string("Removed")
            .with_condition(&unfulfilled_condition)
            .build();
        let choice2 = InternalChoiceBuilder::from_string("Kept")
            .with_condition(&fulfilled_condition)
            .build();
        let choice3 = InternalChoiceBuilder::from_string("Removed")
            .with_condition(&unfulfilled_condition)
            .build();

        let choices = vec![
            create_choice_extra(0, choice1),
            create_choice_extra(0, choice2),
            create_choice_extra(0, choice3),
        ];

        let displayed_choices =
            prepare_choices_for_user(&choices, &current_address, &knots).unwrap();

        assert_eq!(displayed_choices.len(), 1);
        assert_eq!(&displayed_choices[0].text, "Kept");
    }

    #[test]
    fn preparing_choices_filters_choices_which_have_been_visited_for_non_sticky_lines() {
        let choice1 = InternalChoiceBuilder::from_string("Kept").build();
        let choice2 = InternalChoiceBuilder::from_string("Removed").build();
        let choice3 = InternalChoiceBuilder::from_string("Kept").build();

        let choices = vec![
            create_choice_extra(0, choice1),
            create_choice_extra(1, choice2),
            create_choice_extra(0, choice3),
        ];

        let (empty_address, empty_hash_map) = get_mock_address_and_knots();
        let displayed_choices =
            prepare_choices_for_user(&choices, &empty_address, &empty_hash_map).unwrap();

        assert_eq!(displayed_choices.len(), 2);
        assert_eq!(&displayed_choices[0].text, "Kept");
        assert_eq!(&displayed_choices[1].text, "Kept");
    }

    #[test]
    fn preparing_choices_does_not_filter_visited_sticky_lines() {
        let choice1 = InternalChoiceBuilder::from_string("Kept").build();
        let choice2 = InternalChoiceBuilder::from_string("Removed").build();
        let choice3 = InternalChoiceBuilder::from_string("Kept")
            .is_sticky()
            .build();

        let choices = vec![
            create_choice_extra(0, choice1),
            create_choice_extra(1, choice2),
            create_choice_extra(1, choice3),
        ];

        let (empty_address, empty_hash_map) = get_mock_address_and_knots();
        let displayed_choices =
            prepare_choices_for_user(&choices, &empty_address, &empty_hash_map).unwrap();

        assert_eq!(displayed_choices.len(), 2);
        assert_eq!(&displayed_choices[0].text, "Kept");
        assert_eq!(&displayed_choices[1].text, "Kept");
    }

    #[test]
    fn preparing_choices_filters_fallback_choices() {
        let choice1 = InternalChoiceBuilder::from_string("Kept").build();
        let choice2 = InternalChoiceBuilder::from_string("Removed")
            .is_fallback()
            .build();
        let choice3 = InternalChoiceBuilder::from_string("Kept")
            .is_sticky()
            .build();

        let choices = vec![
            create_choice_extra(0, choice1),
            create_choice_extra(0, choice2),
            create_choice_extra(0, choice3),
        ];

        let (empty_address, empty_hash_map) = get_mock_address_and_knots();
        let displayed_choices =
            prepare_choices_for_user(&choices, &empty_address, &empty_hash_map).unwrap();

        assert_eq!(displayed_choices.len(), 2);
        assert_eq!(&displayed_choices[0].text, "Kept");
        assert_eq!(&displayed_choices[1].text, "Kept");
    }

    #[test]
    fn invalid_choice_error_is_filled_in_with_all_presented_choices() {
        let choice1 = InternalChoiceBuilder::from_string("Choice 1").build();
        let choice2 = InternalChoiceBuilder::from_string("Choice 2").build();

        let internal_choices = vec![
            create_choice_extra(0, choice1),
            create_choice_extra(1, choice2),
        ];

        let made_choice = Choice {
            text: "Made this choice".to_string(),
            tags: Vec::new(),
            index: 5,
        };

        let error = InklingError::InvalidChoice {
            index: 2,
            choice: None,
            presented_choices: Vec::new(),
            internal_choices: internal_choices.clone(),
        };

        let (empty_address, empty_hash_map) = get_mock_address_and_knots();
        let filled_error =
            fill_in_invalid_error(error.clone(), &made_choice, &empty_address, &empty_hash_map);

        match (filled_error, error) {
            (
                InklingError::InvalidChoice {
                    index: filled_index,
                    choice: filled_choice,
                    presented_choices: filled_presented_choices,
                    internal_choices: filled_internal_choices,
                },
                InklingError::InvalidChoice {
                    index,
                    internal_choices,
                    ..
                },
            ) => {
                assert_eq!(filled_index, index);
                assert_eq!(filled_internal_choices, internal_choices);

                assert_eq!(filled_choice, Some(made_choice));
                assert_eq!(filled_presented_choices.len(), 2);

                let (shown1, choice1) = &filled_presented_choices[0];
                assert!(shown1);
                assert_eq!(choice1.text, "Choice 1");

                let (shown2, choice2) = &filled_presented_choices[1];
                assert!(!shown2);
                assert_eq!(choice2.text, "Choice 2");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn fallback_choices_are_filtered_as_usual_choices() {
        let choice1 = InternalChoiceBuilder::from_string("Kept")
            .is_fallback()
            .build();
        let choice2 = InternalChoiceBuilder::from_string("Removed")
            .is_fallback()
            .build();
        let choice3 = InternalChoiceBuilder::from_string("Kept")
            .is_sticky()
            .is_fallback()
            .build();

        let choices = vec![
            create_choice_extra(0, choice1),
            create_choice_extra(1, choice2),
            create_choice_extra(1, choice3),
        ];

        let (empty_address, empty_hash_map) = get_mock_address_and_knots();
        let fallback_choices =
            get_fallback_choices(&choices, &empty_address, &empty_hash_map).unwrap();

        assert_eq!(fallback_choices.len(), 2);
        assert_eq!(&fallback_choices[0].text, "Kept");
        assert_eq!(&fallback_choices[1].text, "Kept");
    }
}
