use crate::types::ChannelType;

mod extras;
mod messages;

pub use extras::ExtrasChannel;
pub use messages::MessagesChannel;

pub trait Channel<T>: Sync + Send {
    fn get_channel_type(&self) -> ChannelType;
    fn snapshot(&self) -> T;
    fn len(&self) -> usize;
    fn version(&self) -> u32;
    fn set_version(&mut self, version: u32) -> ();
    fn get_mut(&mut self) -> &mut T;
    fn persistent(&self) -> bool;
}
