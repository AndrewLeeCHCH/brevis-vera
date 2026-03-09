use std::{
    fs,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use brevis_vera_lib::{
    C2PA_STATE_TRUSTED, C2PA_STATE_UNKNOWN, C2PA_STATE_VALID, CropParams, EditWitness, OP_CROP,
    PROVENANCE_MODE_C2PA, PROVENANCE_MODE_MOCK, ProofPublicValues, SignedMetadata, apply_edits,
    op_mask_from_witness, sha256_bytes,
};
use c2pa::jumbf_io::load_jumbf_from_file;
use c2pa::{Reader, ValidationState};
use clap::{Args, Parser, Subcommand};
use image::{GrayImage, imageops::FilterType};
use p256::{
    PublicKey,
    ecdsa::{
        Signature, SigningKey, VerifyingKey,
        signature::{Signer, Verifier},
    },
    pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding},
};
use pico_sdk::{client::DefaultProverClient, init_logger};
use pico_vm::{
    compiler::riscv::program::Program,
    configs::config::StarkGenericConfig,
    configs::stark_config::KoalaBearPoseidon2,
    machine::proof::MetaProof,
    proverchain::{InitialProverSetup, MachineProver, RiscvProver},
};
use rand_core::OsRng;

#[derive(Parser, Debug)]
#[command(author, version, about = "Brevis Vera prototype CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    MockSign(MockSignArgs),
    PrepareSeries(PrepareSeriesArgs),
    EditAndProve(EditAndProveArgs),
    EditAndProveC2pa(EditAndProveC2paArgs),
    Verify(VerifyArgs),
    VerifyC2paProof(VerifyC2paProofArgs),
    VerifyC2pa(VerifyC2paArgs),
    Perf(PerfArgs),
    EmulateCycles(EmulateCyclesArgs),
}

#[derive(Args, Debug)]
struct MockSignArgs {
    #[arg(long)]
    input_image: PathBuf,
    #[arg(long)]
    metadata_out: PathBuf,
    #[arg(long, default_value = "artifacts/mock_signer_key.pem")]
    private_key_pem: PathBuf,
    #[arg(long, default_value = "mock-c2pa-ecdsa-p256")]
    provenance_hint: String,
}

#[derive(Args, Debug)]
struct PrepareSeriesArgs {
    #[arg(long)]
    input_image: PathBuf,
    #[arg(long, default_value = "samples/series")]
    output_dir: PathBuf,
    #[arg(long, default_value = "256,512,1024,2048,4096")]
    sizes: String,
    #[arg(long, default_value = "jpg")]
    ext: String,
}

#[derive(Args, Debug)]
struct EditAndProveArgs {
    #[arg(long)]
    input_image: PathBuf,
    #[arg(long)]
    metadata: PathBuf,
    #[arg(long)]
    crop_x: u32,
    #[arg(long)]
    crop_y: u32,
    #[arg(long)]
    crop_w: u32,
    #[arg(long)]
    crop_h: u32,
    #[arg(long, default_value_t = 15)]
    brightness_delta: i16,
    #[arg(long, default_value_t = false)]
    invert: bool,
    #[arg(long)]
    threshold: Option<u8>,
    #[arg(long, default_value_t = 0)]
    rotate_quarters: u8,
    #[arg(long, default_value = "artifacts/edited.png")]
    edited_image_out: PathBuf,
    #[arg(long, default_value = "artifacts/riscv_proof.bin")]
    riscv_proof_out: PathBuf,
    #[arg(long, default_value = "artifacts/public_values.json")]
    public_values_out: PathBuf,
    #[arg(long, default_value = "app/elf/riscv32im-pico-zkvm-elf")]
    elf: PathBuf,
}

#[derive(Args, Debug)]
struct EditAndProveC2paArgs {
    #[arg(long)]
    input_image: PathBuf,
    #[arg(long)]
    crop_x: u32,
    #[arg(long)]
    crop_y: u32,
    #[arg(long)]
    crop_w: u32,
    #[arg(long)]
    crop_h: u32,
    #[arg(long, default_value_t = 15)]
    brightness_delta: i16,
    #[arg(long, default_value_t = false)]
    invert: bool,
    #[arg(long)]
    threshold: Option<u8>,
    #[arg(long, default_value_t = 0)]
    rotate_quarters: u8,
    #[arg(long, default_value = "artifacts/c2pa_edited.png")]
    edited_image_out: PathBuf,
    #[arg(long, default_value = "artifacts/c2pa_riscv_proof.bin")]
    riscv_proof_out: PathBuf,
    #[arg(long, default_value = "artifacts/c2pa_public_values.json")]
    public_values_out: PathBuf,
    #[arg(long, default_value = "app/elf/riscv32im-pico-zkvm-elf")]
    elf: PathBuf,
}

#[derive(Args, Debug)]
struct VerifyArgs {
    #[arg(long)]
    edited_image: PathBuf,
    #[arg(long)]
    metadata: PathBuf,
    #[arg(long)]
    riscv_proof: PathBuf,
    #[arg(long, default_value = "app/elf/riscv32im-pico-zkvm-elf")]
    elf: PathBuf,
}

#[derive(Args, Debug)]
struct VerifyC2paProofArgs {
    #[arg(long)]
    source_c2pa_image: PathBuf,
    #[arg(long)]
    edited_image: PathBuf,
    #[arg(long)]
    riscv_proof: PathBuf,
    #[arg(long, default_value = "app/elf/riscv32im-pico-zkvm-elf")]
    elf: PathBuf,
}

#[derive(Args, Debug)]
struct VerifyC2paArgs {
    #[arg(long)]
    input_image: PathBuf,
}

#[derive(Args, Debug)]
struct PerfArgs {
    #[arg(long)]
    input_image: PathBuf,
    #[arg(long)]
    metadata: PathBuf,
    #[arg(long)]
    crop_x: u32,
    #[arg(long)]
    crop_y: u32,
    #[arg(long)]
    crop_w: u32,
    #[arg(long)]
    crop_h: u32,
    #[arg(long, default_value_t = 15)]
    brightness_delta: i16,
    #[arg(long, default_value_t = false)]
    invert: bool,
    #[arg(long)]
    threshold: Option<u8>,
    #[arg(long, default_value_t = 0)]
    rotate_quarters: u8,
    #[arg(long, default_value = "app/elf/riscv32im-pico-zkvm-elf")]
    elf: PathBuf,
    #[arg(long, default_value_t = 3)]
    iterations: u32,
    #[arg(long, default_value_t = 1)]
    warmup: u32,
    #[arg(long)]
    c2pa_image: Option<PathBuf>,
    #[arg(long)]
    json_out: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct EmulateCyclesArgs {
    #[arg(long)]
    input_image: PathBuf,
    #[arg(long)]
    metadata: PathBuf,
    #[arg(long)]
    crop_x: u32,
    #[arg(long)]
    crop_y: u32,
    #[arg(long)]
    crop_w: u32,
    #[arg(long)]
    crop_h: u32,
    #[arg(long, default_value_t = 15)]
    brightness_delta: i16,
    #[arg(long, default_value_t = false)]
    invert: bool,
    #[arg(long)]
    threshold: Option<u8>,
    #[arg(long, default_value_t = 0)]
    rotate_quarters: u8,
    #[arg(long, default_value = "app/elf/riscv32im-pico-zkvm-elf")]
    elf: PathBuf,
    #[arg(long, default_value_t = 3)]
    iterations: u32,
    #[arg(long, default_value_t = 1)]
    warmup: u32,
}

#[derive(serde::Serialize)]
struct PublicValuesJson {
    original_hash_hex: String,
    edited_hash_hex: String,
    op_mask: u8,
    provenance_mode: u8,
    provenance_manifest_hash_hex: String,
    provenance_asset_hash_hex: String,
    provenance_state: u8,
}

#[derive(Clone, Default)]
struct StageStats {
    values_ms: Vec<f64>,
}

#[derive(serde::Serialize)]
struct StageSummary {
    stage: String,
    runs: usize,
    min_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    avg_ms: f64,
    max_ms: f64,
}

#[derive(serde::Serialize)]
struct PerfReport {
    iterations: u32,
    warmup: u32,
    cycles: CycleSummary,
    summaries: Vec<StageSummary>,
}

#[derive(serde::Serialize)]
struct CycleSummary {
    runs: usize,
    min: u64,
    p50: u64,
    p95: u64,
    avg: f64,
    max: u64,
}

fn main() -> Result<()> {
    init_logger();

    let cli = Cli::parse();

    match cli.command {
        Commands::MockSign(args) => run_mock_sign(args),
        Commands::PrepareSeries(args) => run_prepare_series(args),
        Commands::EditAndProve(args) => run_edit_and_prove(args),
        Commands::EditAndProveC2pa(args) => run_edit_and_prove_c2pa(args),
        Commands::Verify(args) => run_verify(args),
        Commands::VerifyC2paProof(args) => run_verify_c2pa_proof(args),
        Commands::VerifyC2pa(args) => run_verify_c2pa(args),
        Commands::Perf(args) => run_perf(args),
        Commands::EmulateCycles(args) => run_emulate_cycles(args),
    }
}

fn run_mock_sign(args: MockSignArgs) -> Result<()> {
    ensure_parent_dir(&args.metadata_out)?;
    ensure_parent_dir(&args.private_key_pem)?;

    let (_, _, pixels) = load_grayscale(&args.input_image)?;
    let image_hash = sha256_bytes(&pixels);

    let signing_key = load_or_generate_key(&args.private_key_pem)?;
    let verifying_key = signing_key.verifying_key();
    let signature: Signature = signing_key.sign(&image_hash);

    let metadata = SignedMetadata {
        scheme: "ecdsa-p256-sha256-mock-provenance-v1".to_string(),
        image_hash_hex: hex::encode(image_hash),
        signer_pubkey_sec1_hex: hex::encode(verifying_key.to_encoded_point(false).as_bytes()),
        signature_der_hex: hex::encode(signature.to_der().as_bytes()),
        issued_at_unix_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system time is before UNIX epoch")?
            .as_secs(),
        provenance_hint: args.provenance_hint,
    };

    fs::write(&args.metadata_out, serde_json::to_vec_pretty(&metadata)?)
        .with_context(|| format!("failed writing {}", args.metadata_out.display()))?;

    println!(
        "Mock signature generated: metadata={} key={} hash={}",
        args.metadata_out.display(),
        args.private_key_pem.display(),
        metadata.image_hash_hex
    );

    Ok(())
}

fn run_prepare_series(args: PrepareSeriesArgs) -> Result<()> {
    fs::create_dir_all(&args.output_dir)
        .with_context(|| format!("failed creating {}", args.output_dir.display()))?;

    let sizes = parse_sizes(&args.sizes)?;
    let src = image::open(&args.input_image)
        .with_context(|| format!("failed opening image {}", args.input_image.display()))?;

    let mut written: Vec<PathBuf> = Vec::with_capacity(sizes.len());
    for s in sizes {
        let out = src.resize_exact(s, s, FilterType::Triangle);
        let filename = format!("series_{}x{}.{}", s, s, args.ext);
        let out_path = args.output_dir.join(filename);
        out.save(&out_path)
            .with_context(|| format!("failed writing {}", out_path.display()))?;
        written.push(out_path);
    }

    println!("Prepared {} images:", written.len());
    for p in written {
        println!("- {}", p.display());
    }
    Ok(())
}

fn run_edit_and_prove(args: EditAndProveArgs) -> Result<()> {
    ensure_parent_dir(&args.edited_image_out)?;
    ensure_parent_dir(&args.riscv_proof_out)?;
    ensure_parent_dir(&args.public_values_out)?;

    let (width, height, pixels) = load_grayscale(&args.input_image)?;
    let original_hash = sha256_bytes(&pixels);

    let metadata: SignedMetadata = read_json(&args.metadata)?;
    verify_signed_metadata(&metadata, &original_hash)?;

    let witness = EditWitness {
        original_pixels: pixels,
        original_width: width,
        original_height: height,
        crop: CropParams {
            x: args.crop_x,
            y: args.crop_y,
            width: args.crop_w,
            height: args.crop_h,
        },
        brightness_delta: args.brightness_delta,
        invert: args.invert,
        threshold: args.threshold,
        rotate_quarters: args.rotate_quarters,
        provenance_mode: PROVENANCE_MODE_MOCK,
        provenance_manifest_hash: [0u8; 32],
        provenance_asset_hash: original_hash,
        provenance_state: C2PA_STATE_UNKNOWN,
    };

    let edited = apply_edits(&witness).map_err(anyhow::Error::msg)?;
    save_grayscale(
        &args.edited_image_out,
        edited.width,
        edited.height,
        &edited.pixels,
    )?;

    let edited_hash = sha256_bytes(&edited.pixels);

    let elf = fs::read(&args.elf)
        .with_context(|| format!("failed to read ELF at {}", args.elf.display()))?;
    let client = DefaultProverClient::new(&elf);

    let mut stdin_builder = client.new_stdin_builder();
    stdin_builder.write(&witness);

    let riscv_proof = client.prove_fast(stdin_builder)?;

    riscv_proof.save_to_file(&args.riscv_proof_out)?;

    let pv_stream = riscv_proof
        .pv_stream
        .clone()
        .context("riscv proof missing public value stream")?;
    let public_values: ProofPublicValues = bincode::deserialize(&pv_stream)?;

    if public_values.original_hash != original_hash {
        bail!("proof public original hash does not match signed input");
    }
    if public_values.edited_hash != edited_hash {
        bail!("proof public edited hash does not match generated output");
    }
    let expected_op_mask = op_mask_from_witness(&witness);
    if public_values.op_mask != expected_op_mask {
        bail!("proof op_mask does not match expected operations mask");
    }
    if public_values.provenance_mode != PROVENANCE_MODE_MOCK {
        bail!("expected mock provenance mode in proof");
    }
    if public_values.provenance_asset_hash != original_hash {
        bail!("proof provenance asset hash does not match source hash");
    }

    let public_values_json = PublicValuesJson {
        original_hash_hex: hex::encode(public_values.original_hash),
        edited_hash_hex: hex::encode(public_values.edited_hash),
        op_mask: public_values.op_mask,
        provenance_mode: public_values.provenance_mode,
        provenance_manifest_hash_hex: hex::encode(public_values.provenance_manifest_hash),
        provenance_asset_hash_hex: hex::encode(public_values.provenance_asset_hash),
        provenance_state: public_values.provenance_state,
    };

    fs::write(
        &args.public_values_out,
        serde_json::to_vec_pretty(&public_values_json)?,
    )
    .with_context(|| format!("failed writing {}", args.public_values_out.display()))?;

    println!(
        "Attestation artifacts written:\n- edited={}\n- riscv_proof={}\n- public_values={}\nAUTHENTICITY VERDICT: VALID",
        args.edited_image_out.display(),
        args.riscv_proof_out.display(),
        args.public_values_out.display()
    );

    Ok(())
}

fn run_edit_and_prove_c2pa(args: EditAndProveC2paArgs) -> Result<()> {
    ensure_parent_dir(&args.edited_image_out)?;
    ensure_parent_dir(&args.riscv_proof_out)?;
    ensure_parent_dir(&args.public_values_out)?;

    let reader = Reader::from_file(&args.input_image).with_context(|| {
        format!(
            "failed to parse C2PA data from {}",
            args.input_image.display()
        )
    })?;
    let state = reader.validation_state();
    if state == ValidationState::Invalid {
        bail!("C2PA validation state is invalid");
    }

    let manifest_store_bytes = load_jumbf_from_file(&args.input_image).with_context(|| {
        format!(
            "failed extracting C2PA JUMBF from {}",
            args.input_image.display()
        )
    })?;
    let manifest_hash = sha256_bytes(&manifest_store_bytes);

    let (width, height, pixels) = load_grayscale(&args.input_image)?;
    let original_hash = sha256_bytes(&pixels);

    let witness = EditWitness {
        original_pixels: pixels,
        original_width: width,
        original_height: height,
        crop: CropParams {
            x: args.crop_x,
            y: args.crop_y,
            width: args.crop_w,
            height: args.crop_h,
        },
        brightness_delta: args.brightness_delta,
        invert: args.invert,
        threshold: args.threshold,
        rotate_quarters: args.rotate_quarters,
        provenance_mode: PROVENANCE_MODE_C2PA,
        provenance_manifest_hash: manifest_hash,
        provenance_asset_hash: original_hash,
        provenance_state: map_validation_state(state),
    };

    let edited = apply_edits(&witness).map_err(anyhow::Error::msg)?;
    save_grayscale(
        &args.edited_image_out,
        edited.width,
        edited.height,
        &edited.pixels,
    )?;
    let edited_hash = sha256_bytes(&edited.pixels);

    let elf = fs::read(&args.elf)
        .with_context(|| format!("failed to read ELF at {}", args.elf.display()))?;
    let client = DefaultProverClient::new(&elf);

    let mut stdin_builder = client.new_stdin_builder();
    stdin_builder.write(&witness);
    let riscv_proof = client.prove_fast(stdin_builder)?;
    riscv_proof.save_to_file(&args.riscv_proof_out)?;

    let pv_stream = riscv_proof
        .pv_stream
        .clone()
        .context("riscv proof missing public value stream")?;
    let public_values: ProofPublicValues = bincode::deserialize(&pv_stream)?;

    if public_values.original_hash != original_hash {
        bail!("proof public original hash does not match source image hash");
    }
    if public_values.edited_hash != edited_hash {
        bail!("proof public edited hash does not match generated output");
    }
    let expected_op_mask = op_mask_from_witness(&witness);
    if public_values.op_mask != expected_op_mask {
        bail!("proof op_mask does not match expected operations mask");
    }
    if public_values.provenance_mode != PROVENANCE_MODE_C2PA {
        bail!("proof provenance mode is not C2PA");
    }
    if public_values.provenance_manifest_hash != manifest_hash {
        bail!("proof provenance manifest hash mismatch");
    }
    if public_values.provenance_asset_hash != original_hash {
        bail!("proof provenance asset hash mismatch");
    }

    let public_values_json = PublicValuesJson {
        original_hash_hex: hex::encode(public_values.original_hash),
        edited_hash_hex: hex::encode(public_values.edited_hash),
        op_mask: public_values.op_mask,
        provenance_mode: public_values.provenance_mode,
        provenance_manifest_hash_hex: hex::encode(public_values.provenance_manifest_hash),
        provenance_asset_hash_hex: hex::encode(public_values.provenance_asset_hash),
        provenance_state: public_values.provenance_state,
    };
    fs::write(
        &args.public_values_out,
        serde_json::to_vec_pretty(&public_values_json)?,
    )
    .with_context(|| format!("failed writing {}", args.public_values_out.display()))?;

    println!(
        "C2PA attestation artifacts written:\n- edited={}\n- riscv_proof={}\n- public_values={}\nAUTHENTICITY VERDICT: VALID",
        args.edited_image_out.display(),
        args.riscv_proof_out.display(),
        args.public_values_out.display()
    );
    Ok(())
}

fn run_verify(args: VerifyArgs) -> Result<()> {
    let metadata: SignedMetadata = read_json(&args.metadata)?;

    let (_, _, edited_pixels) = load_grayscale(&args.edited_image)?;
    let edited_hash = sha256_bytes(&edited_pixels);

    type RiscvProof = MetaProof<KoalaBearPoseidon2>;
    let riscv_proof: RiscvProof = RiscvProof::load_from_file(&args.riscv_proof)?;

    let elf = fs::read(&args.elf)
        .with_context(|| format!("failed to read ELF at {}", args.elf.display()))?;
    let riscv = RiscvProver::<KoalaBearPoseidon2, Program>::new_initial_prover(
        (KoalaBearPoseidon2::new(), &elf),
        Default::default(),
        None,
    );
    if !riscv.verify(&riscv_proof, riscv.vk()) {
        bail!("pico riscv proof verification failed");
    }

    let pv_stream = riscv_proof
        .pv_stream
        .clone()
        .context("riscv proof missing public value stream")?;
    let public_values: ProofPublicValues = bincode::deserialize(&pv_stream)?;

    verify_signed_metadata(&metadata, &public_values.original_hash)?;

    if public_values.edited_hash != edited_hash {
        bail!("edited image hash does not match proof public hash");
    }
    if public_values.provenance_mode != PROVENANCE_MODE_MOCK {
        bail!("expected mock provenance mode");
    }
    if public_values.provenance_asset_hash != public_values.original_hash {
        bail!("mock provenance asset hash mismatch");
    }
    if public_values.op_mask & OP_CROP == 0 {
        bail!("proof does not attest mandatory crop transformation");
    }

    println!(
        "AUTHENTICITY VERDICT: VALID\n- signature: valid\n- proof: valid\n- linkage: signed original hash and edited output hash both match"
    );

    Ok(())
}

fn run_verify_c2pa_proof(args: VerifyC2paProofArgs) -> Result<()> {
    let reader = Reader::from_file(&args.source_c2pa_image).with_context(|| {
        format!(
            "failed to parse C2PA data from {}",
            args.source_c2pa_image.display()
        )
    })?;
    let state = reader.validation_state();
    if state == ValidationState::Invalid {
        bail!("source C2PA image validation is invalid");
    }
    let manifest_store_bytes =
        load_jumbf_from_file(&args.source_c2pa_image).with_context(|| {
            format!(
                "failed extracting C2PA JUMBF from {}",
                args.source_c2pa_image.display()
            )
        })?;
    let manifest_hash = sha256_bytes(&manifest_store_bytes);

    let (_, _, source_pixels) = load_grayscale(&args.source_c2pa_image)?;
    let source_hash = sha256_bytes(&source_pixels);
    let (_, _, edited_pixels) = load_grayscale(&args.edited_image)?;
    let edited_hash = sha256_bytes(&edited_pixels);

    type RiscvProof = MetaProof<KoalaBearPoseidon2>;
    let riscv_proof: RiscvProof = RiscvProof::load_from_file(&args.riscv_proof)?;

    let elf = fs::read(&args.elf)
        .with_context(|| format!("failed to read ELF at {}", args.elf.display()))?;
    let riscv = RiscvProver::<KoalaBearPoseidon2, Program>::new_initial_prover(
        (KoalaBearPoseidon2::new(), &elf),
        Default::default(),
        None,
    );
    if !riscv.verify(&riscv_proof, riscv.vk()) {
        bail!("pico riscv proof verification failed");
    }

    let pv_stream = riscv_proof
        .pv_stream
        .clone()
        .context("riscv proof missing public value stream")?;
    let public_values: ProofPublicValues = bincode::deserialize(&pv_stream)?;

    if public_values.provenance_mode != PROVENANCE_MODE_C2PA {
        bail!("proof provenance mode is not C2PA");
    }
    if public_values.provenance_manifest_hash != manifest_hash {
        bail!("proof provenance manifest hash mismatch");
    }
    if public_values.provenance_asset_hash != source_hash {
        bail!("proof provenance asset hash mismatch");
    }
    if public_values.provenance_state != map_validation_state(state) {
        bail!("proof provenance state mismatch");
    }
    if public_values.original_hash != source_hash {
        bail!("proof original hash mismatch with source image");
    }
    if public_values.edited_hash != edited_hash {
        bail!("proof edited hash mismatch with edited image");
    }
    if public_values.op_mask & OP_CROP == 0 {
        bail!("proof does not attest mandatory crop transformation");
    }

    println!(
        "AUTHENTICITY VERDICT: VALID\n- c2pa: valid\n- proof: valid\n- linkage: C2PA manifest and source hash bound inside proof"
    );
    Ok(())
}

fn run_verify_c2pa(args: VerifyC2paArgs) -> Result<()> {
    let reader = Reader::from_file(&args.input_image).with_context(|| {
        format!(
            "failed to parse C2PA data from {}",
            args.input_image.display()
        )
    })?;

    let state = reader.validation_state();
    let active_label = reader.active_label().unwrap_or("none");
    let mut failure_codes: Vec<String> = Vec::new();
    if let Some(statuses) = reader.validation_status() {
        for s in statuses {
            if !s.passed() {
                failure_codes.push(s.code().to_string());
            }
        }
    }

    println!("C2PA active manifest: {}", active_label);
    println!("C2PA validation state: {:?}", state);
    if failure_codes.is_empty() {
        println!("C2PA failure statuses: none");
    } else {
        println!("C2PA failure statuses: {}", failure_codes.join(", "));
    }

    match state {
        ValidationState::Invalid => bail!("C2PA signature/manifest validation is invalid"),
        ValidationState::Valid | ValidationState::Trusted => {
            println!("C2PA VERDICT: VALID");
            Ok(())
        }
    }
}

fn run_perf(args: PerfArgs) -> Result<()> {
    if args.iterations == 0 {
        bail!("iterations must be > 0");
    }

    let metadata: SignedMetadata = read_json(&args.metadata)?;
    let elf = fs::read(&args.elf)
        .with_context(|| format!("failed to read ELF at {}", args.elf.display()))?;
    let client = DefaultProverClient::new(&elf);
    let riscv = RiscvProver::<KoalaBearPoseidon2, Program>::new_initial_prover(
        (KoalaBearPoseidon2::new(), &elf),
        Default::default(),
        None,
    );

    let mut load_image_stats = StageStats::default();
    let mut verify_sig_stats = StageStats::default();
    let mut edit_stats = StageStats::default();
    let mut prove_stats = StageStats::default();
    let mut emulate_stats = StageStats::default();
    let mut verify_proof_stats = StageStats::default();
    let mut linkage_stats = StageStats::default();
    let mut total_stats = StageStats::default();
    let mut c2pa_stats = StageStats::default();
    let mut cycles: Vec<u64> = Vec::new();

    let total_runs = args.warmup + args.iterations;
    for i in 0..total_runs {
        let keep = i >= args.warmup;

        let total_start = Instant::now();

        let t0 = Instant::now();
        let (width, height, pixels) = load_grayscale(&args.input_image)?;
        let original_hash = sha256_bytes(&pixels);
        if keep {
            load_image_stats.push(t0.elapsed());
        }

        let t1 = Instant::now();
        verify_signed_metadata(&metadata, &original_hash)?;
        if keep {
            verify_sig_stats.push(t1.elapsed());
        }

        let witness = EditWitness {
            original_pixels: pixels,
            original_width: width,
            original_height: height,
            crop: CropParams {
                x: args.crop_x,
                y: args.crop_y,
                width: args.crop_w,
                height: args.crop_h,
            },
            brightness_delta: args.brightness_delta,
            invert: args.invert,
            threshold: args.threshold,
            rotate_quarters: args.rotate_quarters,
            provenance_mode: PROVENANCE_MODE_MOCK,
            provenance_manifest_hash: [0u8; 32],
            provenance_asset_hash: original_hash,
            provenance_state: C2PA_STATE_UNKNOWN,
        };

        let t2 = Instant::now();
        let edited = apply_edits(&witness).map_err(anyhow::Error::msg)?;
        if keep {
            edit_stats.push(t2.elapsed());
        }
        let edited_hash = sha256_bytes(&edited.pixels);

        let t3 = Instant::now();
        let mut stdin_builder = client.new_stdin_builder();
        stdin_builder.write(&witness);
        let riscv_proof = client.prove_fast(stdin_builder)?;
        if keep {
            prove_stats.push(t3.elapsed());
        }

        let te = Instant::now();
        let mut emu_stdin_builder = client.new_stdin_builder();
        emu_stdin_builder.write(&witness);
        let (reports, _) = client.emulate(emu_stdin_builder);
        let run_cycles = reports.last().map(|r| r.current_cycle).unwrap_or(0);
        if keep {
            emulate_stats.push(te.elapsed());
            cycles.push(run_cycles);
        }

        let t4 = Instant::now();
        if !riscv.verify(&riscv_proof, riscv.vk()) {
            bail!("pico riscv proof verification failed during perf");
        }
        if keep {
            verify_proof_stats.push(t4.elapsed());
        }

        let t5 = Instant::now();
        let pv_stream = riscv_proof
            .pv_stream
            .clone()
            .context("riscv proof missing public value stream")?;
        let public_values: ProofPublicValues = bincode::deserialize(&pv_stream)?;
        if public_values.original_hash != original_hash {
            bail!("proof public original hash does not match signed input");
        }
        if public_values.edited_hash != edited_hash {
            bail!("proof public edited hash does not match generated output");
        }
        let expected_op_mask = op_mask_from_witness(&witness);
        if public_values.op_mask != expected_op_mask {
            bail!("proof op_mask does not match expected operations mask");
        }
        if public_values.provenance_mode != PROVENANCE_MODE_MOCK {
            bail!("proof provenance mode is not mock");
        }
        if public_values.provenance_asset_hash != original_hash {
            bail!("proof provenance asset hash mismatch");
        }
        if keep {
            linkage_stats.push(t5.elapsed());
        }

        if let Some(c2pa_image) = args.c2pa_image.as_ref() {
            let tc = Instant::now();
            let reader = Reader::from_file(c2pa_image).with_context(|| {
                format!("failed to parse C2PA data from {}", c2pa_image.display())
            })?;
            if reader.validation_state() == ValidationState::Invalid {
                bail!("C2PA validation state is invalid during perf");
            }
            if keep {
                c2pa_stats.push(tc.elapsed());
            }
        }

        if keep {
            total_stats.push(total_start.elapsed());
        }
    }

    let mut summaries = vec![
        stage_summary("load_image", &load_image_stats),
        stage_summary("verify_signature", &verify_sig_stats),
        stage_summary("edit_pipeline", &edit_stats),
        stage_summary("prove_fast", &prove_stats),
        stage_summary("emulate_for_cycles", &emulate_stats),
        stage_summary("verify_proof", &verify_proof_stats),
        stage_summary("linkage_check", &linkage_stats),
    ];
    if args.c2pa_image.is_some() {
        summaries.push(stage_summary("verify_c2pa", &c2pa_stats));
    }
    summaries.push(stage_summary("total", &total_stats));

    println!(
        "Performance report (warmup={}, measured={}):",
        args.warmup, args.iterations
    );
    let cycle_summary = summarize_cycles(&cycles);
    println!(
        "Cycle summary: runs={} avg={:.0} p50={} p95={} min={} max={}",
        cycle_summary.runs,
        cycle_summary.avg,
        cycle_summary.p50,
        cycle_summary.p95,
        cycle_summary.min,
        cycle_summary.max
    );
    for s in &summaries {
        println!(
            "- {:16} runs={} avg={:.2}ms p50={:.2}ms p95={:.2}ms min={:.2}ms max={:.2}ms",
            s.stage, s.runs, s.avg_ms, s.p50_ms, s.p95_ms, s.min_ms, s.max_ms
        );
    }

    if let Some(path) = args.json_out.as_ref() {
        ensure_parent_dir(path)?;
        let report = PerfReport {
            iterations: args.iterations,
            warmup: args.warmup,
            cycles: cycle_summary,
            summaries,
        };
        fs::write(path, serde_json::to_vec_pretty(&report)?)
            .with_context(|| format!("failed writing {}", path.display()))?;
        println!("Perf JSON written: {}", path.display());
    }

    Ok(())
}

fn run_emulate_cycles(args: EmulateCyclesArgs) -> Result<()> {
    if args.iterations == 0 {
        bail!("iterations must be > 0");
    }

    let metadata: SignedMetadata = read_json(&args.metadata)?;
    let elf = fs::read(&args.elf)
        .with_context(|| format!("failed to read ELF at {}", args.elf.display()))?;
    let client = DefaultProverClient::new(&elf);

    let total_runs = args.warmup + args.iterations;
    let mut cycles: Vec<u64> = Vec::new();
    let mut emu_stats = StageStats::default();

    for i in 0..total_runs {
        let keep = i >= args.warmup;
        let (width, height, pixels) = load_grayscale(&args.input_image)?;
        let original_hash = sha256_bytes(&pixels);
        verify_signed_metadata(&metadata, &original_hash)?;

        let witness = EditWitness {
            original_pixels: pixels,
            original_width: width,
            original_height: height,
            crop: CropParams {
                x: args.crop_x,
                y: args.crop_y,
                width: args.crop_w,
                height: args.crop_h,
            },
            brightness_delta: args.brightness_delta,
            invert: args.invert,
            threshold: args.threshold,
            rotate_quarters: args.rotate_quarters,
            provenance_mode: PROVENANCE_MODE_MOCK,
            provenance_manifest_hash: [0u8; 32],
            provenance_asset_hash: original_hash,
            provenance_state: C2PA_STATE_UNKNOWN,
        };

        let t = Instant::now();
        let mut stdin_builder = client.new_stdin_builder();
        stdin_builder.write(&witness);
        let (reports, _) = client.emulate(stdin_builder);
        let elapsed = t.elapsed();
        let run_cycles = reports.last().map(|r| r.current_cycle).unwrap_or(0);

        if keep {
            cycles.push(run_cycles);
            emu_stats.push(elapsed);
        }
    }

    let cycle_summary = summarize_cycles(&cycles);
    let emu_summary = stage_summary("emulate", &emu_stats);
    println!(
        "Emulation cycles (warmup={}, measured={}): avg={:.0} p50={} p95={} min={} max={}",
        args.warmup,
        args.iterations,
        cycle_summary.avg,
        cycle_summary.p50,
        cycle_summary.p95,
        cycle_summary.min,
        cycle_summary.max
    );
    println!(
        "Emulation time: avg={:.2}ms p50={:.2}ms p95={:.2}ms min={:.2}ms max={:.2}ms",
        emu_summary.avg_ms,
        emu_summary.p50_ms,
        emu_summary.p95_ms,
        emu_summary.min_ms,
        emu_summary.max_ms
    );
    Ok(())
}

impl StageStats {
    fn push(&mut self, d: std::time::Duration) {
        self.values_ms.push(d.as_secs_f64() * 1000.0);
    }
}

fn stage_summary(name: &str, stats: &StageStats) -> StageSummary {
    let mut values = stats.values_ms.clone();
    values.sort_by(|a, b| a.total_cmp(b));

    let runs = values.len();
    let min_ms = values.first().copied().unwrap_or(0.0);
    let max_ms = values.last().copied().unwrap_or(0.0);
    let p50_ms = percentile(&values, 0.50);
    let p95_ms = percentile(&values, 0.95);
    let avg_ms = if runs == 0 {
        0.0
    } else {
        values.iter().sum::<f64>() / (runs as f64)
    };

    StageSummary {
        stage: name.to_string(),
        runs,
        min_ms,
        p50_ms,
        p95_ms,
        avg_ms,
        max_ms,
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[pos]
}

fn summarize_cycles(samples: &[u64]) -> CycleSummary {
    let mut values = samples.to_vec();
    values.sort_unstable();
    let runs = values.len();
    let min = values.first().copied().unwrap_or(0);
    let max = values.last().copied().unwrap_or(0);
    let p50 = percentile_u64(&values, 0.50);
    let p95 = percentile_u64(&values, 0.95);
    let avg = if runs == 0 {
        0.0
    } else {
        (values.iter().map(|v| *v as f64).sum::<f64>()) / (runs as f64)
    };

    CycleSummary {
        runs,
        min,
        p50,
        p95,
        avg,
        max,
    }
}

fn percentile_u64(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let pos = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[pos]
}

fn map_validation_state(state: ValidationState) -> u8 {
    match state {
        ValidationState::Invalid => C2PA_STATE_UNKNOWN,
        ValidationState::Valid => C2PA_STATE_VALID,
        ValidationState::Trusted => C2PA_STATE_TRUSTED,
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("failed reading {}", path.display()))?;
    let parsed = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid JSON in {}", path.display()))?;
    Ok(parsed)
}

fn verify_signed_metadata(metadata: &SignedMetadata, expected_hash: &[u8; 32]) -> Result<()> {
    let metadata_hash = hex::decode(&metadata.image_hash_hex)
        .context("metadata image_hash_hex is not valid hex")?;
    if metadata_hash.as_slice() != expected_hash {
        bail!("metadata hash does not match expected image hash");
    }

    let pk_bytes = hex::decode(&metadata.signer_pubkey_sec1_hex)
        .context("metadata signer_pubkey_sec1_hex is not valid hex")?;
    let sig_bytes = hex::decode(&metadata.signature_der_hex)
        .context("metadata signature_der_hex is not valid hex")?;

    let public_key = PublicKey::from_sec1_bytes(&pk_bytes).context("invalid sec1 public key")?;
    let verifying_key = VerifyingKey::from(public_key);
    let signature = Signature::from_der(&sig_bytes).context("invalid DER signature")?;

    verifying_key
        .verify(expected_hash, &signature)
        .context("ECDSA signature verification failed")?;

    Ok(())
}

fn load_or_generate_key(path: &Path) -> Result<SigningKey> {
    if path.exists() {
        let pem = fs::read_to_string(path)
            .with_context(|| format!("failed reading key PEM {}", path.display()))?;
        return SigningKey::from_pkcs8_pem(&pem).context("invalid private key PEM");
    }

    let key = SigningKey::random(&mut OsRng);
    let pem = key
        .to_pkcs8_pem(LineEnding::LF)
        .context("failed encoding generated key to PEM")?;
    fs::write(path, pem.as_bytes())
        .with_context(|| format!("failed writing generated key {}", path.display()))?;
    Ok(key)
}

fn load_grayscale(path: &Path) -> Result<(u32, u32, Vec<u8>)> {
    let image = image::open(path)
        .with_context(|| format!("failed opening image {}", path.display()))?
        .to_luma8();
    let (w, h) = image.dimensions();
    Ok((w, h, image.into_raw()))
}

fn save_grayscale(path: &Path, width: u32, height: u32, pixels: &[u8]) -> Result<()> {
    let image = GrayImage::from_raw(width, height, pixels.to_vec())
        .context("failed constructing grayscale image from edited pixels")?;
    image
        .save(path)
        .with_context(|| format!("failed saving image {}", path.display()))?;
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating directory {}", parent.display()))?;
    }
    Ok(())
}

fn parse_sizes(raw: &str) -> Result<Vec<u32>> {
    let mut out = Vec::new();
    for p in raw.split(',') {
        let t = p.trim();
        if t.is_empty() {
            continue;
        }
        let v: u32 = t
            .parse()
            .with_context(|| format!("invalid size value '{}'", t))?;
        if v == 0 {
            bail!("size values must be > 0");
        }
        out.push(v);
    }
    if out.is_empty() {
        bail!("sizes list is empty");
    }
    Ok(out)
}
