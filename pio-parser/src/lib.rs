// PIO instr grouping is 3/5/3/5
#![allow(clippy::manual_range_contains)]
#![allow(clippy::unusual_byte_groupings)]
#![allow(clippy::upper_case_acronyms)]

use pio_core::{
    InSource, Instruction, InstructionOperands, IrqIndexMode, JmpCondition, MovDestination,
    MovOperation, MovRxIndex, MovSource, OutDestination, ProgramWithDefines, SetDestination,
    WaitSource,
};

use std::collections::HashMap;

mod parser {
    #![allow(clippy::all)]
    #![allow(unused)]
    include!(concat!(env!("OUT_DIR"), "/pio.rs"));
}

#[derive(Debug)]
pub(crate) enum Value<'input> {
    I32(i32),
    Symbol(&'input str),
    Add(Box<Value<'input>>, Box<Value<'input>>),
    Sub(Box<Value<'input>>, Box<Value<'input>>),
    Mul(Box<Value<'input>>, Box<Value<'input>>),
    Div(Box<Value<'input>>, Box<Value<'input>>),
    Neg(Box<Value<'input>>),
    Rev(Box<Value<'input>>),
}

impl Value<'_> {
    fn reify(&self, state: &ProgramState) -> i32 {
        match self {
            Value::I32(v) => *v,
            Value::Symbol(s) => state.resolve(s),
            Value::Add(a, b) => a.reify(state) + b.reify(state),
            Value::Sub(a, b) => a.reify(state) - b.reify(state),
            Value::Mul(a, b) => a.reify(state) * b.reify(state),
            Value::Div(a, b) => a.reify(state) / b.reify(state),
            Value::Neg(a) => -a.reify(state),
            Value::Rev(a) => a.reify(state).reverse_bits(),
        }
    }
}

#[derive(Debug)]
pub(crate) enum Line<'input> {
    Directive(ParsedDirective<'input>),
    Instruction(ParsedInstruction<'input>),
    Label { public: bool, name: &'input str },
}

#[derive(Debug)]
pub(crate) enum ParsedDirective<'input> {
    Define {
        public: bool,
        name: &'input str,
        value: Value<'input>,
    },
    Origin(Value<'input>),
    SideSet {
        value: Value<'input>,
        opt: bool,
        pindirs: bool,
    },
    WrapTarget,
    Wrap,
    #[allow(unused)]
    LangOpt(&'input str),
}

#[derive(Debug)]
pub(crate) struct ParsedInstruction<'input> {
    operands: ParsedOperands<'input>,
    side_set: Option<Value<'input>>,
    delay: Value<'input>,
}

impl ParsedInstruction<'_> {
    fn reify(&self, state: &ProgramState) -> Instruction {
        Instruction {
            operands: self.operands.reify(state),
            side_set: self.side_set.as_ref().map(|s| s.reify(state) as u8),
            delay: self.delay.reify(state) as u8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ParsedMovDestination {
    PINS,
    X,
    Y,
    PINDIRS,
    EXEC,
    PC,
    ISR,
    OSR,
    RXFIFOY,
    RXFIFO0,
    RXFIFO1,
    RXFIFO2,
    RXFIFO3,
}

#[derive(Debug)]
enum MovDestInternal {
    Mov(MovDestination),
    Fifo(MovRxIndex),
}

impl From<ParsedMovDestination> for MovDestInternal {
    fn from(value: ParsedMovDestination) -> Self {
        match value {
            ParsedMovDestination::PINS => MovDestInternal::Mov(MovDestination::PINS),
            ParsedMovDestination::X => MovDestInternal::Mov(MovDestination::X),
            ParsedMovDestination::Y => MovDestInternal::Mov(MovDestination::Y),
            ParsedMovDestination::PINDIRS => MovDestInternal::Mov(MovDestination::PINDIRS),
            ParsedMovDestination::EXEC => MovDestInternal::Mov(MovDestination::EXEC),
            ParsedMovDestination::PC => MovDestInternal::Mov(MovDestination::PC),
            ParsedMovDestination::ISR => MovDestInternal::Mov(MovDestination::ISR),
            ParsedMovDestination::OSR => MovDestInternal::Mov(MovDestination::OSR),
            ParsedMovDestination::RXFIFOY => MovDestInternal::Fifo(MovRxIndex::RXFIFOY),
            ParsedMovDestination::RXFIFO0 => MovDestInternal::Fifo(MovRxIndex::RXFIFO0),
            ParsedMovDestination::RXFIFO1 => MovDestInternal::Fifo(MovRxIndex::RXFIFO1),
            ParsedMovDestination::RXFIFO2 => MovDestInternal::Fifo(MovRxIndex::RXFIFO2),
            ParsedMovDestination::RXFIFO3 => MovDestInternal::Fifo(MovRxIndex::RXFIFO3),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ParsedMovSource {
    PINS,
    X,
    Y,
    NULL,
    STATUS,
    ISR,
    OSR,
    RXFIFOY,
    RXFIFO0,
    RXFIFO1,
    RXFIFO2,
    RXFIFO3,
}

#[derive(Debug)]
enum MovSrcInternal {
    Mov(MovSource),
    Fifo(MovRxIndex),
}

impl From<ParsedMovSource> for MovSrcInternal {
    fn from(value: ParsedMovSource) -> Self {
        match value {
            ParsedMovSource::PINS => MovSrcInternal::Mov(MovSource::PINS),
            ParsedMovSource::X => MovSrcInternal::Mov(MovSource::X),
            ParsedMovSource::Y => MovSrcInternal::Mov(MovSource::Y),
            ParsedMovSource::NULL => MovSrcInternal::Mov(MovSource::NULL),
            ParsedMovSource::STATUS => MovSrcInternal::Mov(MovSource::STATUS),
            ParsedMovSource::ISR => MovSrcInternal::Mov(MovSource::ISR),
            ParsedMovSource::OSR => MovSrcInternal::Mov(MovSource::OSR),
            ParsedMovSource::RXFIFOY => MovSrcInternal::Fifo(MovRxIndex::RXFIFOY),
            ParsedMovSource::RXFIFO0 => MovSrcInternal::Fifo(MovRxIndex::RXFIFO0),
            ParsedMovSource::RXFIFO1 => MovSrcInternal::Fifo(MovRxIndex::RXFIFO1),
            ParsedMovSource::RXFIFO2 => MovSrcInternal::Fifo(MovRxIndex::RXFIFO2),
            ParsedMovSource::RXFIFO3 => MovSrcInternal::Fifo(MovRxIndex::RXFIFO3),
        }
    }
}

#[derive(Debug)]
pub(crate) enum ParsedOperands<'input> {
    JMP {
        condition: JmpCondition,
        address: Value<'input>,
    },
    WAIT {
        polarity: Value<'input>,
        source: WaitSource,
        index: Value<'input>,
        relative: bool,
    },
    IN {
        source: InSource,
        bit_count: Value<'input>,
    },
    OUT {
        destination: OutDestination,
        bit_count: Value<'input>,
    },
    PUSH {
        if_full: bool,
        block: bool,
    },
    PULL {
        if_empty: bool,
        block: bool,
    },
    MOV {
        destination: ParsedMovDestination,
        op: MovOperation,
        source: ParsedMovSource,
    },
    IRQ {
        clear: bool,
        wait: bool,
        index: Value<'input>,
        index_mode: IrqIndexMode,
    },
    SET {
        destination: SetDestination,
        data: Value<'input>,
    },
}

impl ParsedOperands<'_> {
    fn reify(&self, state: &ProgramState) -> InstructionOperands {
        match self {
            ParsedOperands::JMP { condition, address } => InstructionOperands::JMP {
                condition: *condition,
                address: address.reify(state) as u8,
            },
            ParsedOperands::WAIT {
                polarity,
                source,
                index,
                relative,
            } => InstructionOperands::WAIT {
                polarity: polarity.reify(state) as u8,
                source: *source,
                index: index.reify(state) as u8,
                relative: *relative,
            },
            ParsedOperands::IN { source, bit_count } => InstructionOperands::IN {
                source: *source,
                bit_count: bit_count.reify(state) as u8,
            },
            ParsedOperands::OUT {
                destination,
                bit_count,
            } => InstructionOperands::OUT {
                destination: *destination,
                bit_count: bit_count.reify(state) as u8,
            },
            ParsedOperands::PUSH { if_full, block } => InstructionOperands::PUSH {
                if_full: *if_full,
                block: *block,
            },
            ParsedOperands::PULL { if_empty, block } => InstructionOperands::PULL {
                if_empty: *if_empty,
                block: *block,
            },
            ParsedOperands::MOV {
                destination,
                op,
                source,
            } => {
                let source_internal = (*source).into();
                let dest_internal = (*destination).into();
                match (source_internal, dest_internal) {
                    (MovSrcInternal::Mov(MovSource::ISR), MovDestInternal::Fifo(fifo_index)) => {
                        InstructionOperands::MOVTORX { fifo_index }
                    }
                    (
                        MovSrcInternal::Fifo(fifo_index),
                        MovDestInternal::Mov(MovDestination::OSR),
                    ) => InstructionOperands::MOVFROMRX { fifo_index },
                    (MovSrcInternal::Mov(s), MovDestInternal::Mov(d)) => InstructionOperands::MOV {
                        destination: d,
                        op: *op,
                        source: s,
                    },
                    (d, s) => panic!("Illegal Mov src/dest combination: {:?} {:?}", d, s),
                }
            }
            ParsedOperands::IRQ {
                clear,
                wait,
                index,
                index_mode,
            } => InstructionOperands::IRQ {
                clear: *clear,
                wait: *wait,
                index: index.reify(state) as u8,
                index_mode: *index_mode,
            },
            ParsedOperands::SET { destination, data } => InstructionOperands::SET {
                destination: *destination,
                data: {
                    let arg = data.reify(state);
                    if arg < 0 || arg > 0x1f {
                        panic!("SET argument out of range: {}", arg);
                    }
                    arg as u8
                },
            },
        }
    }
}

#[derive(Debug, Default)]
struct FileState {
    defines: HashMap<String, (bool, i32)>,
}

#[derive(Debug)]
struct ProgramState<'a> {
    file_state: &'a mut FileState,
    defines: HashMap<String, (bool, i32)>,
}

impl<'a> ProgramState<'a> {
    fn new(file_state: &'a mut FileState) -> Self {
        ProgramState {
            file_state,
            defines: HashMap::new(),
        }
    }

    fn resolve(&self, name: &str) -> i32 {
        self.defines
            .get(name)
            .or_else(|| self.file_state.defines.get(name))
            .unwrap_or_else(|| panic!("Unknown label {}", name))
            .1
    }

    fn public_defines(&self) -> HashMap<String, i32> {
        let mut p = HashMap::new();
        for (name, (public, value)) in &self.file_state.defines {
            if *public {
                p.insert(name.to_string(), *value);
            }
        }
        for (name, (public, value)) in &self.defines {
            if *public {
                p.insert(name.to_string(), *value);
            }
        }
        p
    }
}

pub type ParseError<'input> = lalrpop_util::ParseError<usize, parser::Token<'input>, &'static str>;

pub struct Parser<const PROGRAM_SIZE: usize>;

impl<const PROGRAM_SIZE: usize> Parser<PROGRAM_SIZE> {
    /// Parse a PIO "file", which contains some number of PIO programs
    /// separated by `.program` directives.
    pub fn parse_file(
        source: &str,
    ) -> Result<HashMap<String, ProgramWithDefines<HashMap<String, i32>, PROGRAM_SIZE>>, ParseError>
    {
        match parser::FileParser::new().parse(source) {
            Ok(f) => {
                let mut state = FileState::default();

                // set up global defines
                let fake_prog_state = ProgramState::new(&mut state);
                for d in f.0 {
                    if let ParsedDirective::Define {
                        public,
                        name,
                        value,
                    } = d.0
                    {
                        fake_prog_state
                            .file_state
                            .defines
                            .insert(name.to_string(), (public, value.reify(&fake_prog_state)));
                    }
                }

                Ok(f.1
                    .iter()
                    .map(|p| {
                        let program_name = p.0.to_string();
                        (program_name, Parser::process(&p.1, &mut state))
                    })
                    .collect())
            }
            Err(e) => Err(e),
        }
    }

    /// Parse a single PIO program, without the `.program` directive.
    pub fn parse_program(
        source: &str,
    ) -> Result<ProgramWithDefines<HashMap<String, i32>, PROGRAM_SIZE>, ParseError> {
        match parser::ProgramParser::new().parse(source) {
            Ok(p) => Ok(Parser::process(&p, &mut FileState::default())),
            Err(e) => Err(e),
        }
    }

    fn process(
        p: &[Line],
        file_state: &mut FileState,
    ) -> ProgramWithDefines<HashMap<String, i32>, PROGRAM_SIZE> {
        let mut state = ProgramState::new(file_state);

        // first pass
        //   - resolve labels
        //   - resolve defines
        //   - read side set settings
        let mut side_set_size = 0;
        let mut side_set_opt = false;
        let mut side_set_pindirs = false;
        let mut origin = None;
        let mut wrap_target = None;
        let mut wrap = None;
        let mut instr_index = 0;
        for line in p {
            match line {
                Line::Instruction(..) => {
                    instr_index += 1;
                }
                Line::Label { public, name } => {
                    state
                        .defines
                        .insert(name.to_string(), (*public, instr_index as i32));
                }
                Line::Directive(d) => match d {
                    ParsedDirective::Define {
                        public,
                        name,
                        value,
                    } => {
                        state
                            .defines
                            .insert(name.to_string(), (*public, value.reify(&state)));
                    }
                    ParsedDirective::Origin(value) => {
                        origin = Some(value.reify(&state) as u8);
                    }
                    ParsedDirective::SideSet {
                        value,
                        opt,
                        pindirs,
                    } => {
                        assert!(instr_index == 0);
                        side_set_size = value.reify(&state) as u8;
                        side_set_opt = *opt;
                        side_set_pindirs = *pindirs;
                    }
                    ParsedDirective::WrapTarget => {
                        assert!(wrap_target.is_none());
                        wrap_target = Some(instr_index);
                    }
                    ParsedDirective::Wrap => {
                        assert!(wrap.is_none());
                        wrap = Some(instr_index - 1);
                    }
                    _ => {}
                },
            }
        }

        let mut a = pio_core::Assembler::new_with_side_set(pio_core::SideSet::new(
            side_set_opt,
            side_set_size,
            side_set_pindirs,
        ));

        // second pass
        //   - emit instructions
        for line in p {
            if let Line::Instruction(i) = line {
                a.instructions.push(i.reify(&state));
            }
        }

        let program = a.assemble_program().set_origin(origin);

        let program = match (wrap, wrap_target) {
            (Some(wrap_source), Some(wrap_target)) => program.set_wrap(pio_core::Wrap {
                source: wrap_source,
                target: wrap_target,
            }),
            (None, None) => program,
            _ => panic!(
                "must define either both or neither of wrap and wrap_target, but not only one of them"
            ),
        };

        ProgramWithDefines {
            program,
            public_defines: state.public_defines(),
        }
    }
}

#[test]
fn test() {
    let p = Parser::<32>::parse_program(
        "
    label:
      pull
      out pins, 1
      jmp label
    ",
    )
    .unwrap();

    assert_eq!(
        &p.program.code[..],
        &[
            // LABEL:
            0b100_00000_101_00000, // PULL
            0b011_00000_000_00001, // OUT PINS, 1
            0b000_00000_000_00000, // JMP LABEL
        ]
    );
    assert_eq!(p.program.origin, None);
    assert_eq!(
        p.program.wrap,
        pio_core::Wrap {
            source: 2,
            target: 0,
        }
    );
}

#[test]
fn test_rp2350() {
    let p = Parser::<32>::parse_program(
        "
    label:
      mov osr, rxfifo[0]
      mov rxfifo[1], isr
      mov pins, isr
      mov osr, x
      jmp label
    ",
    )
    .unwrap();

    assert_eq!(
        &p.program.code[..],
        &[
            // LABEL:
            0b100_00000_1001_1_000, // MOV OSR, RXFIFO0
            0b100_00000_0001_1_001, // MOV RXFIFO1, ISR
            0b101_00000_000_00_110, // MOV PINS, ISR
            0b101_00000_111_00_001, // MOV OSR, X
            0b000_00000_000_00000,  // JMP LABEL
        ]
    );
    assert_eq!(p.program.origin, None);
    assert_eq!(
        p.program.wrap,
        pio_core::Wrap {
            source: 4,
            target: 0,
        }
    );
}

#[test]
fn test_side_set() {
    let p = Parser::<32>::parse_program(
        "
    .side_set 1 opt
    .origin 5

    label:
      pull
      .wrap_target
      out pins, 1
      .wrap
      jmp label side 1
    ",
    )
    .unwrap();

    assert_eq!(
        &p.program.code[..],
        &[
            // LABEL:
            0b100_00000_101_00000, // PULL
            0b011_00000_000_00001, // OUT PINS, 1
            0b000_11000_000_00000, // JMP LABEL, SIDE 1
        ]
    );
    assert_eq!(p.program.origin, Some(5));
    assert_eq!(
        p.program.wrap,
        pio_core::Wrap {
            source: 1,
            target: 1,
        }
    );
}

#[test]
#[should_panic(expected = "Unknown label some_unknown_label")]
fn test_unknown_label() {
    let _ = Parser::<32>::parse_program(
        "
    jmp some_unknown_label
    ",
    )
    .unwrap();
}
