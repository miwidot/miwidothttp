fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only compile protobuf if grpc feature is enabled
    #[cfg(feature = "grpc")]
    {
        tonic_build::compile_protos("proto/cluster.proto")?;
    }
    
    Ok(())
}