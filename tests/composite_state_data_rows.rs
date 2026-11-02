//! A composite child state that carries data may have several rows on one
//! trigger that all read that data, as in a flat table: each row borrows the
//! state data on its own, so an earlier guarded row that is not taken does not
//! hold the borrow across a later row's transition.

use sml::sml;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault;

pub struct Input {
    value: u8,
}

pub struct Poke;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Data(pub u8);

sml! {
    DataRowsKid {
        *"start"_s + completion<Input> / make = Holding(Data),
         Holding(Data) + completion<Input> [is_one] / take_one = X,
         Holding(Data) + completion<Input> / take_other = X,
         Holding(Data) + event<Poke> [poke_is_one] / poke_one = X,
         Holding(Data) + event<Poke> / poke_other = X,
         Holding(Data) + exception<Fault> / recover_one = X,
         Holding(Data) + exception<Fault> / recover_other = X,
    }

    DataRows {
        *"idle"_s + event<Input> = state<DataRowsKid>,
         state<DataRowsKid> + completion<_> = "idle"_s,
    }
}

#[derive(Default)]
struct DataRowsContext {
    log: Vec<String>,
}

impl DataRowsStateMachineContext for DataRowsContext {
    fn make(&mut self, event: &Input) -> Result<Data, Fault> {
        Ok(Data(event.value))
    }

    fn is_one(&self, data: &Data, _: &Input) -> Result<bool, Fault> {
        Ok(data.0 == 1)
    }

    fn poke_is_one(&self, data: &Data, _: &Poke) -> Result<bool, Fault> {
        Ok(data.0 == 1)
    }

    fn take_one(&mut self, data: &Data, _: &Input) -> Result<(), Fault> {
        self.log.push(format!("take_one {}", data.0));
        Ok(())
    }

    fn take_other(&mut self, data: &Data, _: &Input) -> Result<(), Fault> {
        self.log.push(format!("take_other {}", data.0));
        if data.0 == 3 {
            Err(Fault)
        } else {
            Ok(())
        }
    }

    fn poke_one(&mut self, data: &Data, _: &Poke) -> Result<(), Fault> {
        self.log.push(format!("poke_one {}", data.0));
        Ok(())
    }

    fn poke_other(&mut self, data: &Data, _: &Poke) -> Result<(), Fault> {
        self.log.push(format!("poke_other {}", data.0));
        Ok(())
    }

    fn recover_one(&mut self, data: &Data, _: &Fault) -> Result<(), Fault> {
        self.log.push(format!("recover_one {}", data.0));
        Ok(())
    }

    fn recover_other(&mut self, data: &Data, _: &Fault) -> Result<(), Fault> {
        self.log.push(format!("recover_other {}", data.0));
        Ok(())
    }
}

#[test]
fn two_level_child_data_state_rows_on_one_trigger_each_read_the_data() {
    let mut machine = DataRowsStateMachine::new(DataRowsContext::default());
    machine.process_event(Input { value: 1 }).unwrap();
    assert!(machine.is(&DataRowsStates::Idle));
    machine.process_event(Input { value: 2 }).unwrap();
    assert!(machine.is(&DataRowsStates::Idle));
    assert_eq!(machine.context().log, ["take_one 1", "take_other 2"]);
}

#[test]
fn two_level_child_data_state_exception_rows_read_the_data() {
    // The fault from a completion row is caught by the first of two exception
    // rows on the same data-carrying state (the stabilize fault fix).
    let mut machine = DataRowsStateMachine::new(DataRowsContext::default());
    machine.process_event(Input { value: 3 }).unwrap();
    assert!(machine.is(&DataRowsStates::Idle));
    assert_eq!(machine.context().log, ["take_other 3", "recover_one 3"]);
}

pub struct EnterOther;

sml! {
    MultiDataKid {
        *"start"_s + completion<Input> / multi_make = Holding(Data),
         Holding(Data) + completion<Input> [multi_is_one] / multi_one = X,
         Holding(Data) + completion<Input> / multi_other = X,
    }

    MultiDataSibling {
        *"waiting"_s + event<EnterOther> = X,
    }

    MultiDataRows {
        *"idle"_s + event<Input> = state<MultiDataKid>,
         "idle"_s + event<EnterOther> = state<MultiDataSibling>,
         state<MultiDataKid> + completion<_> = "idle"_s,
         state<MultiDataSibling> + completion<_> = "idle"_s,
    }
}

#[derive(Default)]
struct MultiDataContext {
    log: Vec<String>,
}

impl MultiDataRowsStateMachineContext for MultiDataContext {
    fn multi_make(&mut self, event: &Input) -> Result<Data, ()> {
        Ok(Data(event.value))
    }

    fn multi_is_one(&self, data: &Data, _: &Input) -> Result<bool, ()> {
        Ok(data.0 == 1)
    }

    fn multi_one(&mut self, data: &Data, _: &Input) -> Result<(), ()> {
        self.log.push(format!("one {}", data.0));
        Ok(())
    }

    fn multi_other(&mut self, data: &Data, _: &Input) -> Result<(), ()> {
        self.log.push(format!("other {}", data.0));
        Ok(())
    }
}

#[test]
fn multi_child_data_state_rows_on_one_trigger_each_read_the_data() {
    let mut machine = MultiDataRowsStateMachine::new(MultiDataContext::default());
    machine.process_event(Input { value: 1 }).unwrap();
    assert!(machine.is(&MultiDataRowsStates::Idle));
    machine.process_event(Input { value: 5 }).unwrap();
    assert!(machine.is(&MultiDataRowsStates::Idle));
    assert_eq!(machine.context().log, ["one 1", "other 5"]);
}
