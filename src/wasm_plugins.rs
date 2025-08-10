use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use wasmtime::{Engine, Instance, Module, Store, TypedFunc, Linker};
use wasmtime_wasi::WasiCtxBuilder;

pub struct WasmRuntime {
    engine: Engine,
    plugins: HashMap<String, Plugin>,
}

struct Plugin {
    module: Module,
    name: String,
    version: String,
}

impl WasmRuntime {
    pub fn new() -> Result<Self> {
        let engine = Engine::default();
        
        Ok(Self {
            engine,
            plugins: HashMap::new(),
        })
    }
    
    pub async fn load_plugin(&mut self, name: &str, path: PathBuf) -> Result<()> {
        let module = Module::from_file(&self.engine, path)?;
        
        let plugin = Plugin {
            module,
            name: name.to_string(),
            version: "1.0.0".to_string(),
        };
        
        self.plugins.insert(name.to_string(), plugin);
        
        Ok(())
    }
    
    pub async fn execute_plugin(
        &self,
        plugin_name: &str,
        function_name: &str,
        input: &[u8],
    ) -> Result<Vec<u8>> {
        let plugin = self.plugins
            .get(plugin_name)
            .ok_or_else(|| anyhow::anyhow!("Plugin not found: {}", plugin_name))?;
        
        // Create a new store for this execution
        let wasi = WasiCtxBuilder::new()
            .inherit_stdio()
            .build();
        
        let mut store = Store::new(&self.engine, wasi);
        let mut linker = Linker::new(&self.engine);
        
        // Add WASI to the linker
        wasmtime_wasi::add_to_linker(&mut linker, |s| s)?;
        
        // Instantiate the module
        let instance = linker.instantiate(&mut store, &plugin.module)?;
        
        // Get the function
        let func = instance.get_typed_func::<(i32, i32), i32>(&mut store, function_name)?;
        
        // Allocate memory for input
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow::anyhow!("Memory export not found"))?;
        
        let input_ptr = 0;
        memory.write(&mut store, input_ptr, input)?;
        
        // Call the function
        let result_ptr = func.call(&mut store, (input_ptr as i32, input.len() as i32))?;
        
        // Read the result
        let mut result = vec![0u8; 1024]; // Assume max 1KB result
        memory.read(&store, result_ptr as usize, &mut result)?;
        
        Ok(result)
    }
    
    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        self.plugins.values().map(|p| PluginInfo {
            name: p.name.clone(),
            version: p.version.clone(),
        }).collect()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
}

// Example plugin interface
pub trait WasmPlugin {
    fn on_request(&mut self, request: &[u8]) -> Result<Vec<u8>>;
    fn on_response(&mut self, response: &[u8]) -> Result<Vec<u8>>;
    fn get_info(&self) -> PluginInfo;
}