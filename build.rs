fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only compile protobuf if tonic is being used
    #[cfg(feature = "grpc")]
    {
        // Create proto directory if it doesn't exist
        std::fs::create_dir_all("proto")?;
        
        // Write the cluster proto file
        std::fs::write(
            "proto/cluster.proto",
            include_str!("src/cluster/grpc.rs")
                .lines()
                .skip_while(|l| !l.contains("syntax = \"proto3\""))
                .take_while(|l| !l.contains("\"#;"))
                .collect::<Vec<_>>()
                .join("\n")
        )?;
        
        tonic_build::compile_protos("proto/cluster.proto")?;
    }
    
    Ok(())
}