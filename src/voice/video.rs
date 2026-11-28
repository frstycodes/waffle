// H.264 video: frames reassembled from RTP (the rtp crate's RFC 6184 depacketizer, in our own
// reorder buffer), OpenH264 decode to BGRA, and BGRA encode (OpenH264, or Media Foundation on
// Windows). Sending packetizes with the rtp crate's payloader (media.rs).
// ponytail: H.264 only, and it is the only video codec we advertise, so Discord should not send
// VP8/VP9/AV1 to us. Add those decoders (libvpx, dav1d) if streams arrive in another codec.
// ponytail: software encode (OpenH264, also portable to Windows/Linux); VideoToolbox hardware
// encode on macOS if CPU use while streaming matters.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use bytes::Bytes;
use openh264::{
    OpenH264API,
    decoder::Decoder,
    encoder::{BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, UsageType},
    formats::{BgraSliceU8, YUVBuffer, YUVSource},
};
use rtp::{codecs::h264::H264Packet, packet::Packet, packetizer::Depacketizer};

const START_CODE: [u8; 4] = [0, 0, 0, 1];
const NAL_IDR: u8 = 5;

// One decoded picture. Pixels are BGRA, the order GPUI's RenderImage expects.
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl VideoFrame {
    // Shrunk to fit `width` x `height` by nearest sample, keeping even sides for H.264; one that already fits is copied.
    pub fn fit(&self, width: u32, height: u32) -> VideoFrame {
        let scale = (f64::from(width) / f64::from(self.width)).min(f64::from(height) / f64::from(self.height)).min(1.0);
        let (w, h) = (((f64::from(self.width) * scale) as u32 & !1).max(2), ((f64::from(self.height) * scale) as u32 & !1).max(2));
        let bgra = (0..h)
            .flat_map(|y| (0..w).map(move |x| ((y * self.height / h) * self.width + x * self.width / w) as usize * 4))
            .flat_map(|at| self.bgra.get(at..at + 4).unwrap_or(&[0; 4]).to_owned())
            .collect();
        VideoFrame { width: w, height: h, bgra }
    }
}

// How long a frame with a missing packet waits for its retransmission (RTX, after a NACK) before it
// counts as lost and decoding waits for the next keyframe.
const MAX_WAIT: Duration = Duration::from_millis(250);
// A sequence this far behind the next frame means the sender restarted; nearer is a late duplicate.
const RESTART_GAP: u64 = 1000;
const MAX_PACKETS: usize = 4096;

// A picture's RTP packets wait here, by sequence, until their frame is whole: reordering is absorbed
// and a gap gets MAX_WAIT for its retransmission. A frame comes out as soon as its marker packet
// closes a complete run, as in libwebrtc's packet buffer. (webrtc-media's SampleBuilder holds each
// frame until the next one starts: a frame of latency, and up to the idle repeat on a still screen.)
#[derive(Default)]
pub struct FrameAssembler {
    // By extended (unwrapped) sequence number, with arrival time.
    packets: BTreeMap<u64, (Packet, Instant)>,
    highest: Option<u64>,
    // The first sequence of the next frame.
    next: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Lost;

impl FrameAssembler {
    pub fn push(&mut self, packet: Packet, now: Instant) {
        let seq = self.extend(packet.header.sequence_number);
        if let Some(next) = self.next
            && seq < next
        {
            if seq + RESTART_GAP < next {
                *self = Self::default();
                self.push(packet, now);
            }
            return;
        }
        self.highest = Some(self.highest.map_or(seq, |h| h.max(seq)));
        self.next.get_or_insert(seq);
        self.packets.entry(seq).or_insert((packet, now));
        if self.packets.len() > MAX_PACKETS {
            *self = Self::default();
        }
    }

    // The next frame in order, as Annex-B: Some(Ok) when whole, Some(Err) once given up on, None
    // while waiting.
    pub fn pop(&mut self, now: Instant) -> Option<Result<Vec<u8>, Lost>> {
        let next = self.next?;
        let end = self.packets.range(next..).find(|(_, (p, _))| p.header.marker).map(|(&seq, _)| seq)?;
        let frame = self.packets.range(next..=end);
        let whole = frame.clone().count() as u64 == end - next + 1;
        if !whole && frame.clone().all(|(_, (_, at))| now.duration_since(*at) < MAX_WAIT) {
            return None;
        }
        let unit = whole.then(|| assemble(frame.map(|(_, (p, _))| &p.payload))).flatten();
        self.packets.retain(|&seq, _| seq > end);
        self.next = Some(end + 1);
        Some(unit.ok_or(Lost))
    }

    // The 16-bit sequence unwrapped against the highest one seen.
    fn extend(&self, sequence: u16) -> u64 {
        let Some(highest) = self.highest else { return 1 << 32 | sequence as u64 };
        let delta = sequence.wrapping_sub(highest as u16) as i16;
        highest.saturating_add_signed(delta as i64)
    }
}

// One frame's payloads through the rtp crate's H.264 depacketizer. It doesn't check that a
// fragmented NAL's first fragment was there, so a frame must start at a partition head: one that
// doesn't is a NAL we joined halfway through.
fn assemble<'a>(mut payloads: impl Iterator<Item = &'a Bytes>) -> Option<Vec<u8>> {
    let mut depacketizer = H264Packet::default();
    let first = payloads.next()?;
    if !depacketizer.is_partition_head(first) {
        return None;
    }
    let mut unit = depacketizer.depacketize(first).ok()?.to_vec();
    for payload in payloads {
        unit.extend_from_slice(&depacketizer.depacketize(payload).ok()?);
    }
    (!unit.is_empty()).then_some(unit)
}

pub struct VideoDecoder {
    decoder: Decoder,
    awaiting_keyframe: bool,
}

impl VideoDecoder {
    pub fn new() -> Result<Self, String> {
        Ok(Self { decoder: Decoder::new().map_err(|e| e.to_string())?, awaiting_keyframe: true })
    }

    // A dropped frame breaks the reference chain, so frames are skipped until the next IDR.
    pub fn frame_lost(&mut self) {
        self.awaiting_keyframe = true;
    }

    pub fn awaiting_keyframe(&self) -> bool {
        self.awaiting_keyframe
    }

    pub fn decode(&mut self, unit: &[u8]) -> Option<VideoFrame> {
        if self.awaiting_keyframe && !has_idr(unit) {
            return None;
        }
        self.awaiting_keyframe = false;
        let yuv = match self.decoder.decode(unit) {
            Ok(yuv) => yuv?,
            Err(_) => {
                self.awaiting_keyframe = true;
                return None;
            }
        };
        let (width, height) = yuv.dimensions();
        let mut bgra = vec![0u8; yuv.rgba8_len()];
        yuv.write_rgba8(&mut bgra);
        bgra.as_chunks_mut::<4>().0.iter_mut().for_each(|px| px.swap(0, 2));
        Some(VideoFrame { width: width as u32, height: height as u32, bgra })
    }
}

// On Windows the first frame opens a Media Foundation hardware encoder (video_mf.rs) at that size;
// OpenH264 takes over if there is none, it fails, or WAFFLE_VIDEO_ENCODER=software. A size change
// reopens the hardware encoder; OpenH264 follows size changes by itself.
pub struct VideoEncoder {
    // Reopening an encoder after a size change or a hardware failure, which only Windows does.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    fps: u32,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    bitrate: u32,
    backend: Backend,
    keyframe_pending: bool,
}

enum Backend {
    #[cfg(target_os = "windows")]
    Undecided,
    #[cfg(target_os = "windows")]
    Hardware(super::video_mf::MfEncoder),
    Software(Box<Encoder>),
}

impl VideoEncoder {
    pub fn new(fps: u32, bitrate: u32) -> Result<Self, String> {
        #[cfg(target_os = "windows")]
        let backend = if std::env::var("WAFFLE_VIDEO_ENCODER").as_deref() == Ok("software") { Backend::Software(Box::new(software(fps, bitrate)?)) } else { Backend::Undecided };
        #[cfg(not(target_os = "windows"))]
        let backend = Backend::Software(Box::new(software(fps, bitrate)?));
        Ok(Self { fps, bitrate, backend, keyframe_pending: false })
    }

    pub fn force_keyframe(&mut self) {
        match &mut self.backend {
            Backend::Software(encoder) => encoder.force_intra_frame(),
            // Applied once the encoder for the next frame exists.
            #[cfg(target_os = "windows")]
            _ => self.keyframe_pending = true,
        }
    }

    /// Which encoder is in use: "OpenH264", the hardware encoder's name, or "undecided" before the first frame.
    #[cfg(test)]
    pub fn backend_name(&self) -> String {
        match &self.backend {
            Backend::Software(_) => "OpenH264".into(),
            #[cfg(target_os = "windows")]
            Backend::Hardware(encoder) => encoder.name.clone(),
            #[cfg(target_os = "windows")]
            Backend::Undecided => "undecided".into(),
        }
    }

    // One Annex-B access unit, or None when the encoder skipped or is still holding the frame.
    pub fn encode(&mut self, frame: &VideoFrame) -> Option<Vec<u8>> {
        #[cfg(target_os = "windows")]
        self.ensure_hardware(frame);
        let keyframe = std::mem::take(&mut self.keyframe_pending);
        match &mut self.backend {
            Backend::Software(encoder) => {
                if keyframe {
                    encoder.force_intra_frame();
                }
                let size = (frame.width as usize, frame.height as usize);
                let yuv = YUVBuffer::from_rgb8_source(BgraSliceU8::new(&frame.bgra, size));
                let unit = encoder.encode(&yuv).ok()?.to_vec();
                (!unit.is_empty()).then_some(unit)
            }
            #[cfg(target_os = "windows")]
            Backend::Hardware(encoder) => {
                if keyframe {
                    encoder.force_keyframe();
                }
                match encoder.encode(frame) {
                    Ok(unit) => unit,
                    Err(e) => {
                        eprintln!("voice: {} failed ({e}); switching to OpenH264", encoder.name);
                        self.backend = Backend::Software(Box::new(software(self.fps, self.bitrate).ok()?));
                        self.keyframe_pending = true;
                        self.encode(frame)
                    }
                }
            }
            #[cfg(target_os = "windows")]
            Backend::Undecided => None,
        }
    }

    // Opens the hardware encoder for this frame's size, or settles on OpenH264.
    #[cfg(target_os = "windows")]
    fn ensure_hardware(&mut self, frame: &VideoFrame) {
        let fits = match &self.backend {
            Backend::Undecided => false,
            Backend::Hardware(encoder) => (encoder.width, encoder.height) == (frame.width, frame.height),
            Backend::Software(_) => return,
        };
        if fits {
            return;
        }
        self.backend = match super::video_mf::MfEncoder::new(frame.width, frame.height, self.fps, self.bitrate) {
            Ok(encoder) => {
                if super::debug() {
                    eprintln!("voice: video encoder: {} at {}x{}", encoder.name, frame.width, frame.height);
                }
                Backend::Hardware(encoder)
            }
            Err(e) => {
                if super::debug() {
                    eprintln!("voice: no hardware H.264 encoder ({e}); using OpenH264");
                }
                match software(self.fps, self.bitrate) {
                    Ok(encoder) => Backend::Software(Box::new(encoder)),
                    Err(_) => return,
                }
            }
        };
        // A new encoder starts with a keyframe anyway; a stale request would only cost bandwidth.
        self.keyframe_pending = false;
    }
}

// A keyframe every few seconds lets late viewers start even if keyframe requests are lost.
fn software(fps: u32, bitrate: u32) -> Result<Encoder, String> {
    let config = EncoderConfig::new()
        .usage_type(UsageType::ScreenContentRealTime)
        .adaptive_quantization(false)
        .background_detection(false)
        .bitrate(BitRate::from_bps(bitrate))
        .max_frame_rate(FrameRate::from_hz(fps as f32))
        .intra_frame_period(IntraFramePeriod::from_num_frames(fps * 3));
    Encoder::with_api_config(OpenH264API::from_source(), config).map_err(|e| e.to_string())
}

/// The NAL unit types in an Annex-B access unit, in order.
// Used by the Windows hardware encoder (video_mf.rs) and the tests.
#[cfg(any(test, target_os = "windows"))]
pub fn nal_types(unit: &[u8]) -> Vec<u8> {
    nal_units(unit).iter().filter_map(|nal| nal.first().map(|h| h & 0x1f)).collect()
}

/// The access unit rebuilt with 4-byte start codes, leaving out NALs of the given types.
pub fn without_nal_types(unit: &[u8], drop: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(unit.len());
    for nal in nal_units(unit) {
        if nal.first().is_some_and(|h| !drop.contains(&(h & 0x1f))) {
            out.extend_from_slice(&START_CODE);
            out.extend_from_slice(nal);
        }
    }
    out
}

fn has_idr(unit: &[u8]) -> bool {
    nal_units(unit).iter().any(|nal| nal.first().is_some_and(|h| h & 0x1f == NAL_IDR))
}

// NAL units of an Annex-B stream, without start codes. A zero before `00 00 01` belongs to a
// 4-byte start code, not the previous NAL — the same split DAVE uses, so bytes stay identical.
fn nal_units(unit: &[u8]) -> Vec<&[u8]> {
    let starts: Vec<usize> = unit.windows(3).enumerate().filter(|(_, w)| *w == [0, 0, 1]).map(|(i, _)| i).collect();
    starts
        .iter()
        .enumerate()
        .map(|(n, &at)| {
            let end = starts.get(n + 1).map_or(unit.len(), |&next| if next > 0 && unit[next - 1] == 0 { next - 1 } else { next });
            &unit[at + 3..end.max(at + 3)]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use openh264::{
        encoder::Encoder,
        formats::{RgbSliceU8, YUVBuffer},
    };

    use super::*;

    #[test]
    fn drops_delimiter_and_filler_nals() {
        let unit = [0, 0, 0, 1, 0x09, 0xf0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x65, 2, 0, 0, 0, 1, 0x0c, 0xff, 0xff];
        assert_eq!(without_nal_types(&unit, &[9, 12]), [0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x65, 2]);
    }

    // How media.rs sends a unit: delimiters and filler out, then the rtp crate's payloader.
    fn send(unit: &[u8], mtu: usize, first_sequence: u16, timestamp: u32) -> Vec<Packet> {
        use rtp::{codecs::h264::H264Payloader, header::Header, packetizer::Payloader};
        let payloads = H264Payloader::default().payload(mtu, &Bytes::from(without_nal_types(unit, &[9, 12]))).unwrap();
        let last = payloads.len() - 1;
        let header = |n: usize| Header { version: 2, marker: n == last, payload_type: 101, sequence_number: first_sequence.wrapping_add(n as u16), timestamp, ssrc: 1, ..Default::default() };
        payloads.into_iter().enumerate().map(|(n, payload)| Packet { header: header(n), payload }).collect()
    }

    fn receive(packets: impl IntoIterator<Item = Packet>, now: Instant) -> Vec<Result<Vec<u8>, Lost>> {
        let mut frames = FrameAssembler::default();
        let mut out = Vec::new();
        for packet in packets {
            frames.push(packet, now);
            out.extend(std::iter::from_fn(|| frames.pop(now)));
        }
        out
    }

    fn single(sequence: u16, marker: bool, payload: &'static [u8]) -> Packet {
        let header = rtp::header::Header { version: 2, marker, sequence_number: sequence, ..Default::default() };
        Packet { header, payload: Bytes::from_static(payload) }
    }

    #[test]
    fn reordered_packets_make_the_frame_as_soon_as_it_is_whole() {
        let mut unit = vec![0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x68, 3, 4, 0, 0, 0, 1, 0x65];
        unit.extend(1..=40);
        let mut packets = send(&unit, 20, 65_534, 0);
        assert!(packets.len() > 3, "fragments, and wraps the sequence");
        packets.swap(1, 2);
        assert_eq!(receive(packets, Instant::now()), [Ok(unit)], "out the moment its last packet lands, not with the next frame");
    }

    #[test]
    fn a_gap_that_isnt_filled_loses_only_its_frame() {
        let (mut frames, now) = (FrameAssembler::default(), Instant::now());
        frames.push(single(10, false, &[0x7c, 0x85, 1]), now);
        frames.push(single(12, true, &[0x7c, 0x45, 2]), now);
        frames.push(single(13, true, &[0x41, 3, 3]), now);
        assert_eq!(frames.pop(now), None, "waits for 11");
        let later = now + MAX_WAIT;
        assert_eq!(frames.pop(later), Some(Err(Lost)));
        assert_eq!(frames.pop(later), Some(Ok(vec![0, 0, 0, 1, 0x41, 3, 3])));
        frames.push(single(11, false, &[0x7c, 0x05, 2]), later);
        assert_eq!(frames.pop(later), None, "a late duplicate is ignored");
    }

    #[test]
    fn joining_mid_nal_or_a_restarted_sender_recovers() {
        let now = Instant::now();
        let got = receive([single(5, true, &[0x7c, 0x45, 9]), single(60_000, true, &[0x41, 1, 1])], now);
        assert_eq!(got, [Err(Lost), Ok(vec![0, 0, 0, 1, 0x41, 1, 1])], "a fragment without its start, then a sequence restarted far behind");
    }

    // A moving gradient so every frame differs, like scrolling content.
    fn moving_frame(w: u32, h: u32, i: u32) -> VideoFrame {
        let bgra: Vec<u8> = (0..w * h).flat_map(|p| { let x = (p % w + i * 8) as u8; let y = (p / w) as u8; [x, y, x ^ y, 255] }).collect();
        VideoFrame { width: w, height: h, bgra }
    }

    // `cargo test --release --example voice_join -- --ignored bench_ --nocapture`. On Windows set
    // WAFFLE_VIDEO_ENCODER=software to time OpenH264 instead of the hardware encoder.
    #[test]
    #[ignore]
    fn bench_720p_encode() {
        bench_encode(1280, 720);
    }

    #[test]
    #[ignore]
    fn bench_1080p_encode() {
        bench_encode(1920, 1080);
    }

    fn bench_encode(w: u32, h: u32) {
        let mut encoder = VideoEncoder::new(30, 2_500_000).unwrap();
        let frames: Vec<VideoFrame> = (0..90).map(|i| moving_frame(w, h, i)).collect();
        // The first frame opens the encoder; time steady state only.
        encoder.encode(&frames[0]);
        let (mut total, mut units, mut bytes) = (std::time::Duration::ZERO, 0, 0);
        for frame in &frames[1..] {
            let t = std::time::Instant::now();
            if let Some(unit) = encoder.encode(frame) {
                units += 1;
                bytes += unit.len();
            }
            total += t.elapsed();
        }
        let n = frames.len() as u32 - 1;
        eprintln!("BENCH {} {w}x{h}: avg {:?} per frame (incl. colour conversion), {units}/{n} frames out, {} KB/frame", encoder.backend_name(), total / n, bytes / units.max(1) / 1024);
    }

    // Whatever encoder `VideoEncoder` picks (hardware on Windows when present) must produce streams
    // our own receive path accepts: Annex-B with SPS/PPS before the IDR, no B-frames (output in
    // input order), RTP round trip byte for byte, decodable by OpenH264, keyframes on request.
    #[test]
    fn chosen_encoder_output_survives_the_receive_path() {
        let (w, h) = (320, 240);
        let mut encoder = VideoEncoder::new(30, 1_000_000).unwrap();
        let mut decoder = VideoDecoder::new().unwrap();
        let mut decoded = 0;
        let mut units = Vec::new();
        for i in 0..30 {
            if i == 20 {
                encoder.force_keyframe();
            }
            let Some(unit) = encoder.encode(&moving_frame(w, h, i)) else { continue };
            let [Ok(rebuilt)] = &receive(send(&unit, 1200, i as u16 * 100, i * 3000), Instant::now())[..] else { panic!("unit {i} didn't survive RTP") };
            assert_eq!(rebuilt, &without_nal_types(&unit, &[9, 12]), "byte for byte, as DAVE needs");
            if decoder.decode(rebuilt).is_some_and(|f| (f.width, f.height) == (w, h)) {
                decoded += 1;
            }
            units.push(nal_types(&unit));
        }
        eprintln!("{}: {} units, {decoded} decoded, first {:?}", encoder.backend_name(), units.len(), units.first());
        assert!(units.iter().flatten().all(|t| ![9, 12].contains(t) || encoder.backend_name() == "OpenH264"), "no delimiters or filler on the wire");
        let first = units.first().expect("encoder produced output");
        assert!(first.contains(&7) && first.contains(&8) && first.contains(&5), "first unit is SPS+PPS+IDR: {first:?}");
        assert!(units.iter().skip(1).any(|t| t.contains(&5)), "a requested keyframe arrives");
        assert!(decoded + 2 >= units.len(), "nearly every unit decodes ({decoded}/{})", units.len());
    }

    #[test]
    fn fit_shrinks_to_the_rung_and_keeps_even_sides() {
        let frame = VideoFrame { width: 1280, height: 720, bgra: vec![7; 1280 * 720 * 4] };
        let small = frame.fit(640, 360);
        assert_eq!((small.width, small.height, small.bgra.len()), (640, 360, 640 * 360 * 4));
        assert_eq!(small.bgra[0], 7);
        let odd = frame.fit(301, 301);
        assert_eq!((odd.width % 2, odd.height % 2), (0, 0));
        assert_eq!((frame.fit(4000, 4000).width, frame.fit(4000, 4000).height), (1280, 720), "never enlarged");
    }

    #[test]
    fn encode_packetize_depacketize_decode_round_trip() {
        let (width, height) = (96, 64);
        let bgra: Vec<u8> = (0..width * height).flat_map(|i| [(i % 251) as u8, 40, 200, 255]).collect();
        let frame = VideoFrame { width, height, bgra };
        let mut encoder = VideoEncoder::new(30, 500_000).unwrap();
        let unit = encoder.encode(&frame).unwrap();

        let packets = send(&unit, 100, 0, 0);
        assert!(packets.iter().all(|p| p.payload.len() <= 100));
        assert!(packets.iter().any(|p| p.payload[0] & 0x1f == 28), "small MTU forces FU-A fragmentation");
        let [Ok(rebuilt)] = &receive(packets, Instant::now())[..] else { panic!("frame didn't survive RTP") };
        assert_eq!(rebuilt, &unit, "Annex-B survives the RTP round trip byte for byte");

        let decoded = VideoDecoder::new().unwrap().decode(rebuilt).unwrap();
        assert_eq!((decoded.width, decoded.height), (width, height));
    }

    #[test]
    fn decodes_openh264_keyframe_to_bgra() {
        let (w, h) = (64, 48);
        let red: Vec<u8> = [255u8, 0, 0].repeat(w * h);
        let yuv = YUVBuffer::from_rgb8_source(RgbSliceU8::new(&red, (w, h)));
        let mut encoder = Encoder::new().unwrap();
        let unit = encoder.encode(&yuv).unwrap().to_vec();

        let mut decoder = VideoDecoder::new().unwrap();
        assert!(decoder.decode(&[0, 0, 0, 1, 0x41, 0]).is_none(), "non-IDR before a keyframe is skipped");
        let frame = decoder.decode(&unit).unwrap();
        assert_eq!((frame.width, frame.height, frame.bgra.len()), (64, 48, 64 * 48 * 4));
        let px = &frame.bgra[..4];
        assert!(px[2] > 200 && px[0] < 60, "red lands in the R slot of BGRA: {px:?}");
    }
}
