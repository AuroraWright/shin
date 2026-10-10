use glam::{vec4, Vec4};

#[derive(Copy, Clone, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(transparent)]
pub struct UnormColor(pub u32);

impl UnormColor {
    /// Decimal RGB digits, returned in the RGBA byte order used by rendering.
    pub fn from_decimal_rgb(rgb: i32) -> Self {
        fn decimal_to_8bit(v: u8) -> u8 {
            match v {
                0 => 0,
                1 => 0x1c,
                2 => 0x39,
                3 => 0x55,
                4 => 0x71,
                5 => 0x8e,
                6 => 0xaa,
                7 => 0xc6,
                8 => 0xe3,
                9 => 0xff,
                _ => unreachable!(),
            }
        }

        let rgb = rgb.clamp(0, 999);
        Self::from_rgba(
            decimal_to_8bit(((rgb / 100) % 10) as u8),
            decimal_to_8bit(((rgb / 10) % 10) as u8),
            decimal_to_8bit((rgb % 10) as u8),
            0xff,
        )
    }

    pub fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self((r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | ((a as u32) << 24))
    }

    pub fn into_vec4(self) -> Vec4 {
        vec4(
            (self.0 & 0xff) as f32 / 255.0,
            ((self.0 >> 8) & 0xff) as f32 / 255.0,
            ((self.0 >> 16) & 0xff) as f32 / 255.0,
            ((self.0 >> 24) & 0xff) as f32 / 255.0,
        )
    }

    pub const RED: Self = Self(0xff0000ff);
    pub const GREEN: Self = Self(0xff00ff00);
    pub const BLUE: Self = Self(0xffff0000);

    pub const PASTEL_GREEN: Self = Self(0xffc1e1c1);
    pub const PASTEL_PINK: Self = Self(0xffdcd1ff);

    pub const WHITE: Self = Self(0xffffffff);
    pub const BLACK: Self = Self(0xff000000);
}

#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[cfg_attr(feature = "encase", derive(encase::ShaderType))]
#[repr(C)]
pub struct FloatColor4 {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl FloatColor4 {
    /// Creates a new [`FloatColor4`] from an integer of form `0xARGB` (4 bits per channel).
    #[expect(clippy::identity_op)]
    pub fn from_4bpp_property(value: i32) -> Self {
        let alpha = ((value & 0xf000) >> 12) as f32 / 0xf as f32;
        let red = ((value & 0x0f00) >> 8) as f32 / 0xf as f32;
        let green = ((value & 0x00f0) >> 4) as f32 / 0xf as f32;
        let blue = ((value & 0x000f) >> 0) as f32 / 0xf as f32;

        Self::from_rgba(red, green, blue, alpha)
    }

    pub const fn from_unorm(color: UnormColor) -> Self {
        Self {
            r: (color.0 & 0xff) as f32 / 255.0,
            g: ((color.0 >> 8) & 0xff) as f32 / 255.0,
            b: ((color.0 >> 16) & 0xff) as f32 / 255.0,
            a: ((color.0 >> 24) & 0xff) as f32 / 255.0,
        }
    }

    pub const fn from_rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn from_vec4(vec: Vec4) -> Self {
        let [r, g, b, a] = vec.to_array();
        Self { r, g, b, a }
    }

    pub const fn into_vec4(self) -> Vec4 {
        vec4(self.r, self.g, self.b, self.a)
    }

    pub const fn into_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn into_unorm(self) -> UnormColor {
        let [r, g, b, a] = self.into_array().map(|c| (c * 255.0) as u8);
        UnormColor::from_rgba(r, g, b, a)
    }

    pub fn premultiply(self) -> Self {
        let [r, g, b, a] = self.into_array();
        Self::from_rgba(r * a, g * a, b * a, a)
    }

    pub fn with_alpha(self, new_alpha: f32) -> Self {
        Self::from_rgba(self.r, self.g, self.b, new_alpha)
    }

    pub const RED: Self = Self::from_unorm(UnormColor::RED);
    pub const GREEN: Self = Self::from_unorm(UnormColor::GREEN);
    pub const BLUE: Self = Self::from_unorm(UnormColor::BLUE);

    pub const PASTEL_GREEN: Self = Self::from_unorm(UnormColor::PASTEL_GREEN);
    pub const PASTEL_PINK: Self = Self::from_unorm(UnormColor::PASTEL_PINK);

    pub const WHITE: Self = Self::from_unorm(UnormColor::WHITE);
    pub const BLACK: Self = Self::from_unorm(UnormColor::BLACK);
}

impl std::ops::Mul<FloatColor4> for FloatColor4 {
    type Output = FloatColor4;

    fn mul(self, rhs: FloatColor4) -> Self::Output {
        Self::from_vec4(self.into_vec4() * rhs.into_vec4())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_rgb_keeps_red_and_blue_in_render_channel_order() {
        assert_eq!(UnormColor::from_decimal_rgb(900), UnormColor::RED);
        assert_eq!(UnormColor::from_decimal_rgb(90), UnormColor::GREEN);
        assert_eq!(UnormColor::from_decimal_rgb(9), UnormColor::BLUE);
        assert_eq!(UnormColor::from_decimal_rgb(940).into_vec4(), vec4(1.0,113.0/255.0,0.0,1.0));
        assert_eq!(UnormColor::from_decimal_rgb(999), UnormColor::WHITE);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "executes guarded original/1.03 native decimal text color callback"]
    fn reference_native_decimal_text_colors_match_render_channels() {
        use std::{io::Write, process::{Command, Stdio}};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let binary = std::env::temp_dir().join(format!("shin-text-color-oracle-{}",std::process::id()));
        let compile = Command::new("/usr/bin/clang").args(["-O0", "-Wall", "-Wextra"])
            .arg(root.join("reverse-engineering/native-decimal-text-color.c")).arg("-o").arg(&binary)
            .output().unwrap();
        assert!(compile.status.success(), "{}", String::from_utf8_lossy(&compile.stderr));
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let folder = std::path::Path::new(&rom).parent().unwrap();
        let input: String = (0..1000).map(|v|format!("{v}\n")).collect();
        for name in ["uncompressed_executable", "uncompressed_executable_update"] {
            let mut child = Command::new(&binary).arg(folder.join(name))
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            let output = String::from_utf8(output.stdout).unwrap();
            assert_eq!(output.lines().count(),1000);
            for (value,line) in output.lines().enumerate() {
                let mut fields = line.split_whitespace();
                assert_eq!(fields.next().unwrap().parse::<usize>().unwrap(), value);
                let native = u32::from_str_radix(fields.next().unwrap(),16).unwrap();
                let rgba = UnormColor::from_decimal_rgb(value as i32).0;
                let argb = (rgba & 0xff00ff00) | ((rgba & 0xff)<<16) | ((rgba>>16)&0xff);
                assert_eq!(argb & 0xffffff, native & 0xffffff, "{name}:color {value}");
                if value!=0 {assert_eq!(argb,native,"{name}:color {value}");}
            }
            let mut bytes = std::fs::read(folder.join(name)).unwrap();
            let address = if name.ends_with("update") {0xb53b0} else {0xb5450};
            bytes[address+256] ^= 1;
            let altered = std::env::temp_dir().join(format!("shin-text-color-altered-{}",std::process::id()));
            std::fs::write(&altered, bytes).unwrap();
            let rejected = Command::new(&binary).arg(&altered).output().unwrap();
            std::fs::remove_file(altered).unwrap();
            assert_eq!(rejected.status.code(), Some(2));
            eprintln!("{name}:1000 decimal text RGB values match native ARGB channels; altered callback rejected");
        }
        std::fs::remove_file(binary).unwrap();
    }
}
