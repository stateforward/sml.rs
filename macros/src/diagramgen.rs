use crate::parser::event::EventKind;
use crate::parser::*;

/// Generates DOT syntax for a state-machine diagram with Graphviz.
pub fn generate_diagram(sm: &ParsedStateMachine) -> String {
    let transitions = &sm.states_events_mapping;

    let mut diagram_states = sm.states.iter().map(|s| s.0).collect::<Vec<&String>>();
    diagram_states.sort();
    let diagram_states = diagram_states.into_iter();
    let mut diagram_events = vec![];
    let mut diagram_transitions = vec![];
    for (state, event) in transitions {
        for eventmapping in event.values() {
            for transition in &eventmapping.transitions {
                let (event_id, event_label) = match eventmapping.event_kind {
                    EventKind::Normal => {
                        let event = eventmapping.event.to_string();
                        (event.clone(), event)
                    }
                    EventKind::Unexpected if eventmapping.event_wildcard => {
                        ("unexpected_any".to_string(), "unexpected<_>".to_string())
                    }
                    EventKind::Unexpected => (
                        format!("unexpected_{}", eventmapping.event),
                        format!("unexpected<{}>", eventmapping.event),
                    ),
                    EventKind::Completion => (
                        format!("completion_{}", eventmapping.event),
                        format!("completion<{}>", eventmapping.event),
                    ),
                    EventKind::Entry => ("on_entry".to_string(), "on_entry<_>".to_string()),
                    EventKind::Exit => ("on_exit".to_string(), "on_exit<_>".to_string()),
                    EventKind::Exception => ("exception".to_string(), "exception<_>".to_string()),
                };
                diagram_events.push((
                    event_id.clone(),
                    event_label,
                    transition
                        .guard
                        .as_ref()
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "_".to_string()),
                    transition
                        .action
                        .iter()
                        .chain(transition.additional_actions.iter())
                        .map(|action| action.ident.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                ));
                diagram_transitions.push((state, transition.out_state.to_string(), event_id));
            }
        }
    }
    // Sorting is needed to ensure stable (ie not changing between runs of
    // the same sm code) dot file contents. This is needed to ensure stable
    // hash sum, which is used to name unnamed diagrams. If done without sorting,
    // the output is polluted with lots of similar svg files with different names.
    // This ensures that new files will only occur upon changing the structure of the code.
    diagram_events.sort();
    diagram_transitions.sort();

    let state_string = diagram_states
        .map(|s| {
            format!(
                "\t{} [shape=box color=\"red\" fillcolor=\"#ffbb33\" style=filled]",
                s
            )
        })
        .collect::<Vec<String>>();
    let event_string = diagram_events
        .iter()
        .map(|s| {
            format!(
                "\t{0} [shape=box label=\"{1}\\n[{2}] / {3}\"]",
                s.0, s.1, s.2, s.3
            )
        })
        .collect::<Vec<String>>();
    let transition_string = diagram_transitions
        .iter()
        .map(|t| format!("\t{0} -> {1} [color=blue label={2}];", t.0, t.1, t.2))
        .collect::<Vec<String>>();

    format!(
        "digraph G {{
    rankdir=\"LR\";
    node [fontname=Arial];
    edge [fontname=Arial];
    s [shape=circle size=2 color=\"black\" style=filled]
    
    s -> {}
{}

{}

{}
}}",
        sm.starting_state,
        state_string.join("\n"),
        event_string.join("\n"),
        transition_string.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::generate_diagram;
    use crate::parser::{state_machine::StateMachine, ParsedStateMachine};
    use syn::parse_str;

    #[test]
    fn diagram_distinguishes_completion_and_unexpected_triggers() {
        let parsed: StateMachine = parse_str(
            "transitions: {
                *Idle + Start = Step,
                Step + completion<Start> = Done,
                Done + unexpected<Reset> = Error,
                Done + unexpected<_> = Error
            }",
        )
        .unwrap();
        let machine = ParsedStateMachine::new(parsed).unwrap();
        let diagram = generate_diagram(&machine);

        assert!(diagram.contains("completion<Start>"));
        assert!(diagram.contains("unexpected<Reset>"));
        assert!(diagram.contains("unexpected<_>"));
    }
}
