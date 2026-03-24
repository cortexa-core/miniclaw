pub mod registry;
pub mod get_time;
pub mod file_ops;
pub mod edit_file;
pub mod memory_ops;
pub mod system;

use registry::ToolRegistry;

pub fn register_default_tools(registry: &mut ToolRegistry) {
    registry.register(get_time::GetTimeTool);
    registry.register(file_ops::ReadFileTool);
    registry.register(file_ops::WriteFileTool);
    registry.register(file_ops::ListDirTool);
    registry.register(edit_file::EditFileTool);
    registry.register(memory_ops::MemoryStoreTool);
    registry.register(memory_ops::MemoryReadTool);
    registry.register(system::SystemInfoTool);
}
