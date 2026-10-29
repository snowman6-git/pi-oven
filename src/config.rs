use clap::Parser;
use std::{net::IpAddr, path::PathBuf};

#[derive(Parser, Clone, Debug)]
#[command(
    name = "pi-oven",
    version,
    about = "pi 코딩 에이전트를 브라우저와 데스크톱 앱에서 쓰는 웹 UI 서버",
    after_help = "\
예시:
  pi-oven                              이 PC에서만 (http://127.0.0.1:4141)
  pi-oven --lan                        같은 네트워크의 다른 기기에서도 (토큰을 만들어 출력)
  pi-oven --lan --password-file ~/pw   비밀번호로 접속 (파일의 첫 줄)
  pi-oven --lan --token-file ~/token   토큰을 정해서 접속

외부에 열면 pi가 이 PC에서 명령을 실행할 수 있게 되므로, VPN이나 HTTPS 리버스 프록시 뒤에서 쓰세요.
토큰·비밀번호는 `ps`에 보이니 --token/--password보다 환경변수나 -file 옵션을 권합니다."
)]
pub struct Config {
    /// 듣는 주소. 루프백(127.0.0.1)이 아니면 토큰이나 비밀번호가 꼭 필요합니다(없으면 토큰을 만듭니다).
    #[arg(long, env = "PI_OVEN_HOST", default_value = "127.0.0.1", help_heading = "서버")]
    pub host: IpAddr,

    /// 다른 기기에서도 접속하게 열기 (--host 0.0.0.0과 같음).
    #[arg(long, conflicts_with = "host", help_heading = "서버")]
    pub lan: bool,

    /// 포트.
    #[arg(long, env = "PI_OVEN_PORT", default_value_t = 4141, help_heading = "서버")]
    pub port: u16,

    /// 접속 토큰(12자 이상, 영문·숫자·기호). 주소 끝에 #token=… 으로 붙여 접속합니다.
    #[arg(long, env = "PI_OVEN_TOKEN", help_heading = "접근 제어")]
    pub token: Option<String>,

    /// 토큰을 파일에서 읽기(첫 줄).
    #[arg(long, env = "PI_OVEN_TOKEN_FILE", value_name = "FILE", help_heading = "접근 제어")]
    pub token_file: Option<PathBuf>,

    /// 접속 비밀번호(8자 이상, 공백 없는 영문·숫자·기호). 접속 화면에 직접 입력합니다. 토큰과 함께 쓰면 둘 다 통합니다.
    #[arg(long, alias = "passwd", env = "PI_OVEN_PASSWORD", help_heading = "접근 제어")]
    pub password: Option<String>,

    /// 비밀번호를 파일에서 읽기(첫 줄).
    #[arg(long, alias = "passwd-file", env = "PI_OVEN_PASSWORD_FILE", value_name = "FILE", help_heading = "접근 제어")]
    pub password_file: Option<PathBuf>,

    /// 리버스 프록시 뒤에서 실행: 실패 횟수 제한을 접속자별(X-Forwarded-For의 첫 주소)로 셉니다. 프록시가 이 헤더를 직접 채울 때만 켜세요.
    #[arg(long, help_heading = "접근 제어")]
    pub trust_proxy: bool,

    /// 이 서버의 API를 호출해도 되는 다른 브라우저 출처(여러 번 가능). 예: https://pi.example.com
    #[arg(long = "allow-origin", value_name = "ORIGIN", help_heading = "접근 제어")]
    pub allow_origins: Vec<String>,

    /// 실행할 pi 프로그램.
    #[arg(long, env = "PI_OVEN_PI_BIN", default_value = "pi", help_heading = "pi")]
    pub pi_bin: String,

    /// 모든 `pi --mode rpc` 프로세스에 넘길 인자(여러 번 가능). 예: --pi-arg=--approve
    #[arg(long = "pi-arg", allow_hyphen_values = true, value_name = "ARG", help_heading = "pi")]
    pub pi_args: Vec<String>,

    /// pi가 세션을 저장하는 폴더. 기본: $PI_CODING_AGENT_SESSION_DIR 또는 ~/.pi/agent/sessions
    #[arg(long, env = "PI_OVEN_SESSIONS_DIR", value_name = "DIR", help_heading = "pi")]
    pub sessions_dir: Option<PathBuf>,

    /// 내장 권한 확장을 끕니다(자동/수동 전환이 사라집니다).
    #[arg(long, help_heading = "pi")]
    pub no_permission_ext: bool,

    #[arg(skip)]
    pub extension_path: Option<PathBuf>,
}

pub const MIN_TOKEN: usize = 12;
pub const MIN_PASSWORD: usize = 8;

fn first_line(file: &std::path::Path, what: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{what} 파일을 읽을 수 없습니다 ({}): {e}", file.display()))?;
    Ok(text.lines().next().unwrap_or("").trim().to_owned())
}

/// Visible ASCII only: these travel in an HTTP header and in a URL, and a space or a control character in them is a typo.
fn check_secret(value: &str, what: &str, min: usize) -> Result<(), String> {
    if value.chars().count() < min {
        return Err(format!("{what}은(는) {min}자 이상이어야 합니다."));
    }
    if !value.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(format!("{what}에는 공백이나 한글 같은 문자를 쓸 수 없습니다(영문·숫자·기호만)."));
    }
    Ok(())
}

impl Config {
    pub fn listen_host(&self) -> IpAddr {
        if self.lan { IpAddr::from([0, 0, 0, 0]) } else { self.host }
    }

    /// The token and the password, from the option or its file; each checked.
    pub fn secrets(&self) -> Result<(Option<String>, Option<String>), String> {
        let pick = |direct: &Option<String>, file: &Option<PathBuf>, what: &str, min: usize| -> Result<Option<String>, String> {
            let v = match (direct, file) {
                (Some(_), Some(_)) => return Err(format!("{what}: 값과 파일을 함께 줄 수 없습니다.")),
                (Some(v), None) => v.clone(),
                (None, Some(f)) => first_line(f, what)?,
                (None, None) => return Ok(None),
            };
            check_secret(&v, what, min)?;
            Ok(Some(v))
        };
        let token = pick(&self.token, &self.token_file, "토큰", MIN_TOKEN)?;
        let password = pick(&self.password, &self.password_file, "비밀번호", MIN_PASSWORD)?;
        if let (Some(t), Some(p)) = (&token, &password) {
            if t == p {
                return Err("토큰과 비밀번호는 서로 달라야 합니다.".into());
            }
        }
        Ok((token, password))
    }
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

impl Config {
    pub fn resolved_sessions_dir(&self) -> PathBuf {
        self.sessions_dir
            .clone()
            .or_else(|| std::env::var_os("PI_CODING_AGENT_SESSION_DIR").map(PathBuf::from))
            .unwrap_or_else(|| home_dir().join(".pi/agent/sessions"))
    }
}

const PERMISSION_EXT: &str = include_str!("../ext/oven-permission.ts");

/// Writes the bundled pi extension next to the user's cache so `pi --extension` can load it.
pub fn install_extension() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".cache"))
        .join("pi-oven");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("oven-permission.ts");
    std::fs::write(&path, PERMISSION_EXT).ok()?;
    Some(path)
}

/// 16 random bytes from the OS as hex.
pub fn random_hex(bytes: usize) -> String {
    use std::io::Read;
    let mut buf = vec![0u8; bytes];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .expect("read /dev/urandom");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}
