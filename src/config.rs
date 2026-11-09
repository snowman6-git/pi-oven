use clap::{CommandFactory, FromArgMatches, Parser};
use std::{net::IpAddr, path::PathBuf};

#[derive(Parser, Clone, Debug)]
#[command(
    name = "pi-oven",
    version,
    about = "A web UI server for the pi coding agent, for browsers and the desktop app",
    after_help = "\
Examples:
  pi-oven                              this PC only (http://127.0.0.1:4141)
  pi-oven --lan                        other devices on the network too (makes and prints a token)
  pi-oven --lan --password-file ~/pw   sign in with a password (first line of the file)
  pi-oven --lan --token-file ~/token   sign in with a token you choose

Opened to others, pi can run commands on this PC: keep it behind a VPN or an HTTPS reverse proxy.
Tokens and passwords given as options show up in `ps`; prefer the environment variables or the -file options."
)]
pub struct Config {
    /// Address to listen on. Off loopback (127.0.0.1) a token or a password is required (a token is made if neither is given).
    #[arg(long, env = "PI_OVEN_HOST", default_value = "127.0.0.1", help_heading = "Server")]
    pub host: IpAddr,

    /// Open to other devices too (same as --host 0.0.0.0).
    #[arg(long, conflicts_with = "host", help_heading = "Server")]
    pub lan: bool,

    /// Port.
    #[arg(long, env = "PI_OVEN_PORT", default_value_t = 4141, help_heading = "Server")]
    pub port: u16,

    /// Access token (12+ characters: letters, digits, symbols). Add it to the address as #token=….
    #[arg(long, env = "PI_OVEN_TOKEN", help_heading = "Access")]
    pub token: Option<String>,

    /// Read the token from a file (first line).
    #[arg(long, env = "PI_OVEN_TOKEN_FILE", value_name = "FILE", help_heading = "Access")]
    pub token_file: Option<PathBuf>,

    /// Password (8+ characters, no spaces), typed on the sign-in page. With a token too, either works.
    #[arg(long, alias = "passwd", env = "PI_OVEN_PASSWORD", help_heading = "Access")]
    pub password: Option<String>,

    /// Read the password from a file (first line).
    #[arg(long, alias = "passwd-file", env = "PI_OVEN_PASSWORD_FILE", value_name = "FILE", help_heading = "Access")]
    pub password_file: Option<PathBuf>,

    /// Behind a reverse proxy: count failed sign-ins per client (first X-Forwarded-For address). Only when the proxy sets that header itself.
    #[arg(long, help_heading = "Access")]
    pub trust_proxy: bool,

    /// Another browser origin allowed to call this server's API (repeatable). e.g. https://pi.example.com
    #[arg(long = "allow-origin", value_name = "ORIGIN", help_heading = "Access")]
    pub allow_origins: Vec<String>,

    /// The pi program to run.
    #[arg(long, env = "PI_OVEN_PI_BIN", default_value = "pi", help_heading = "pi")]
    pub pi_bin: String,

    /// An argument for every `pi --mode rpc` process (repeatable). e.g. --pi-arg=--approve
    #[arg(long = "pi-arg", allow_hyphen_values = true, value_name = "ARG", help_heading = "pi")]
    pub pi_args: Vec<String>,

    /// Where pi keeps sessions. Default: $PI_CODING_AGENT_SESSION_DIR or ~/.pi/agent/sessions
    #[arg(long, env = "PI_OVEN_SESSIONS_DIR", value_name = "DIR", help_heading = "pi")]
    pub sessions_dir: Option<PathBuf>,

    /// Turn off the built-in permission extension (no auto/manual switch).
    #[arg(long, help_heading = "pi")]
    pub no_permission_ext: bool,

    #[arg(skip)]
    pub extension_path: Option<PathBuf>,

    #[arg(skip)]
    pub retry_extension_path: Option<PathBuf>,
}

/// The command line in the terminal's language: Korean help on a Korean locale, English otherwise.
pub fn parse_args() -> Config {
    let mut cmd = Config::command();
    if crate::lang::cli_korean() {
        cmd = cmd
            .about("pi 코딩 에이전트를 브라우저와 데스크톱 앱에서 쓰는 웹 UI 서버")
            .after_help(
                "\
예시:
  pi-oven                              이 PC에서만 (http://127.0.0.1:4141)
  pi-oven --lan                        같은 네트워크의 다른 기기에서도 (토큰을 만들어 출력)
  pi-oven --lan --password-file ~/pw   비밀번호로 접속 (파일의 첫 줄)
  pi-oven --lan --token-file ~/token   토큰을 정해서 접속

외부에 열면 pi가 이 PC에서 명령을 실행할 수 있게 되므로, VPN이나 HTTPS 리버스 프록시 뒤에서 쓰세요.
토큰·비밀번호는 `ps`에 보이니 --token/--password보다 환경변수나 -file 옵션을 권합니다.",
            );
        let ko: [(&str, &str, &str); 13] = [
            ("host", "서버", "듣는 주소. 루프백(127.0.0.1)이 아니면 토큰이나 비밀번호가 꼭 필요합니다(없으면 토큰을 만듭니다)."),
            ("lan", "서버", "다른 기기에서도 접속하게 열기 (--host 0.0.0.0과 같음)."),
            ("port", "서버", "포트."),
            ("token", "접근 제어", "접속 토큰(12자 이상, 영문·숫자·기호). 주소 끝에 #token=… 으로 붙여 접속합니다."),
            ("token_file", "접근 제어", "토큰을 파일에서 읽기(첫 줄)."),
            ("password", "접근 제어", "접속 비밀번호(8자 이상, 공백 없는 영문·숫자·기호). 접속 화면에 직접 입력합니다. 토큰과 함께 쓰면 둘 다 통합니다."),
            ("password_file", "접근 제어", "비밀번호를 파일에서 읽기(첫 줄)."),
            ("trust_proxy", "접근 제어", "리버스 프록시 뒤에서 실행: 실패 횟수 제한을 접속자별(X-Forwarded-For의 첫 주소)로 셉니다. 프록시가 이 헤더를 직접 채울 때만 켜세요."),
            ("allow_origins", "접근 제어", "이 서버의 API를 호출해도 되는 다른 브라우저 출처(여러 번 가능). 예: https://pi.example.com"),
            ("pi_bin", "pi", "실행할 pi 프로그램."),
            ("pi_args", "pi", "모든 `pi --mode rpc` 프로세스에 넘길 인자(여러 번 가능). 예: --pi-arg=--approve"),
            ("sessions_dir", "pi", "pi가 세션을 저장하는 폴더. 기본: $PI_CODING_AGENT_SESSION_DIR 또는 ~/.pi/agent/sessions"),
            ("no_permission_ext", "pi", "내장 권한 확장을 끕니다(자동/수동 전환이 사라집니다)."),
        ];
        for (id, heading, help) in ko {
            cmd = cmd.mut_arg(id, |a| a.help_heading(heading).help(help));
        }
    }
    let matches = cmd.get_matches();
    Config::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
}

pub const MIN_TOKEN: usize = 12;
pub const MIN_PASSWORD: usize = 8;

fn first_line(file: &std::path::Path, what: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(file).map_err(|e| tr!(format!("{what} 파일을 읽을 수 없습니다 ({}): {e}", file.display()), format!("cannot read the {what} file ({}): {e}", file.display())))?;
    Ok(text.lines().next().unwrap_or("").trim().to_owned())
}

/// Visible ASCII only: these travel in an HTTP header and in a URL, and a space or a control character in them is a typo.
fn check_secret(value: &str, what: &str, min: usize) -> Result<(), String> {
    if value.chars().count() < min {
        return Err(tr!(format!("{what}은(는) {min}자 이상이어야 합니다."), format!("the {what} must be at least {min} characters.")));
    }
    if !value.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(tr!(
            format!("{what}에는 공백이나 한글 같은 문자를 쓸 수 없습니다(영문·숫자·기호만)."),
            format!("the {what} may only use letters, digits and symbols (no spaces or other scripts).")
        ));
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
                (Some(_), Some(_)) => return Err(tr!(format!("{what}: 값과 파일을 함께 줄 수 없습니다."), format!("{what}: give the value or the file, not both."))),
                (Some(v), None) => v.clone(),
                (None, Some(f)) => first_line(f, what)?,
                (None, None) => return Ok(None),
            };
            check_secret(&v, what, min)?;
            Ok(Some(v))
        };
        let token = pick(&self.token, &self.token_file, tr!("토큰", "token"), MIN_TOKEN)?;
        let password = pick(&self.password, &self.password_file, tr!("비밀번호", "password"), MIN_PASSWORD)?;
        if let (Some(t), Some(p)) = (&token, &password) {
            if t == p {
                return Err(tr!("토큰과 비밀번호는 서로 달라야 합니다.", "the token and the password must differ.").into());
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
const RETRY_EXT: &str = include_str!("../ext/oven-retry.ts");

pub fn install_extension() -> Option<PathBuf> {
    install("oven-permission.ts", PERMISSION_EXT)
}

pub fn install_retry_extension() -> Option<PathBuf> {
    install("oven-retry.ts", RETRY_EXT)
}

/// Writes a bundled pi extension next to the user's cache so `pi --extension` can load it.
fn install(name: &str, body: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".cache"))
        .join("pi-oven");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(name);
    std::fs::write(&path, body).ok()?;
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
