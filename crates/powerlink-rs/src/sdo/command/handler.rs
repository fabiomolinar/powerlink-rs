// crates/powerlink-rs/src/sdo/command/handler.rs
use crate::od::ObjectDictionary;
use crate::sdo::command::SdoCommand;

/// A trait for handling optional or vendor-specific SDO commands.
///
/// An application can implement this trait and provide it to the `SdoServer`
/// to add support for commands that are not part of the core implementation.
pub trait SdoCommandHandler {
    /// Handles the WriteAllByIndex command.
    fn handle_write_all_by_index(
        &mut self,
        command: SdoCommand,
        od: &mut ObjectDictionary,
    ) -> SdoCommand;

    /// Handles the WriteMultipleParamByIndex command.
    fn handle_write_multiple_params(
        &mut self,
        command: SdoCommand,
        od: &mut ObjectDictionary,
    ) -> SdoCommand;

    /// Handles the FileRead command.
    fn handle_file_read(&mut self, command: SdoCommand, od: &mut ObjectDictionary) -> SdoCommand;

    /// Handles the FileWrite command.
    fn handle_file_write(&mut self, command: SdoCommand, od: &mut ObjectDictionary) -> SdoCommand;
}

/// A default, no-op implementation that aborts all commands.
pub struct DefaultSdoHandler;

impl SdoCommandHandler for DefaultSdoHandler {
    fn handle_write_all_by_index(
        &mut self,
        command: SdoCommand,
        _od: &mut ObjectDictionary,
    ) -> SdoCommand {
        abort(command, 0x0601_0001) // Unsupported access
    }

    fn handle_write_multiple_params(
        &mut self,
        command: SdoCommand,
        _od: &mut ObjectDictionary,
    ) -> SdoCommand {
        abort(command, 0x0601_0001) // Unsupported access
    }

    fn handle_file_read(&mut self, command: SdoCommand, _od: &mut ObjectDictionary) -> SdoCommand {
        abort(command, 0x0601_0001) // Unsupported access
    }

    fn handle_file_write(&mut self, command: SdoCommand, _od: &mut ObjectDictionary) -> SdoCommand {
        abort(command, 0x0601_0001) // Unsupported access
    }
}

// Helper function to create an abort response
fn abort(command: SdoCommand, abort_code: u32) -> SdoCommand {
    use super::{CommandId, CommandLayerHeader, Segmentation};

    SdoCommand {
        header: CommandLayerHeader {
            transaction_id: command.header.transaction_id,
            is_response: true,
            is_aborted: true,
            segmentation: Segmentation::Expedited,
            command_id: CommandId::Nil,
            segment_size: 4,
        },
        data_size: None,
        payload: abort_code.to_le_bytes().to_vec(),
    }
}