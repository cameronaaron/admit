//! Against the site's real JSON-LD (captured 2026-10-06, trimmed).

use admit_domain::{Experience, ExperienceKind, License, LicenseKind, Profile};
use admit_website::{SiteError, compare, expiry, fetch, parse, published};
use time::macros::date;

const HOME: &str = include_str!("fixtures/home.html");

#[test]
fn reads_every_role_and_deduplicates_credentials() {
    let p = parse(HOME);
    assert_eq!(p.roles.len(), 9);
    assert_eq!(p.credentials.len(), 10, "Person.hasCredential and the list publish the same ten");
    let emt: Vec<_> = p.credentials.iter().filter(|c| c.name.starts_with("Emergency Medical")).collect();
    assert_eq!(emt.len(), 2, "two EMT registrations, told apart by identifier");
    let cna = p.credentials.iter().find(|c| c.name.contains("CNA")).expect("CNA");
    assert_eq!(cna.expires, Some((2028, 9)));
    assert!(published("u", "<html></html>").is_err(), "a page without JSON-LD is an error, not 19 findings");
}

fn license(title: &str, org: &str, until: Option<&str>, id: &str) -> License {
    License {
        kind: LicenseKind::Certification,
        title: title.into(),
        organization: org.into(),
        description: Some(format!("Credential ID: {id}")),
        valid_until: until.map(Into::into),
        ids: Default::default(),
    }
}

fn exp(org: &str, title: &str, start: &str, end: &str) -> Experience {
    Experience {
        kind: ExperienceKind::Employment,
        organization: org.into(),
        title: title.into(),
        state: "California".into(),
        country: "United States".into(),
        start: start.into(),
        end: Some(end.into()),
        current: false,
        status: "Full time".into(),
        hours_per_week: 40,
        weeks: 4,
        total_hours: 160,
        compensated: true,
        volunteer: false,
        description: String::new(),
        supervisor: Default::default(),
        ids: Default::default(),
    }
}

#[test]
fn disagreements_are_reported_in_both_directions() {
    let site = parse(HOME);
    let mut p = Profile::default();
    p.licenses.licenses = vec![
        // Matched by identifier; expiry agrees (Sep 2028).
        license(
            "Certified Nursing Assistant (CNA)",
            "California Dept. of Public Health",
            Some("2028-09-30"),
            "CNA 01357099",
        ),
        // Matched by identifier; expiry disagrees (site: Mar 2028).
        license(
            "Emergency Medical Technician (EMT) - National",
            "National Registry of EMTs",
            Some("2027-03-31"),
            "E4097508",
        ),
        // Only in the profile.
        license("Wilderness First Responder", "NOLS", None, "WFR-1"),
    ];
    p.experiences.experiences = vec![
        exp("Bridges Academy", "Director of Information Technology Engineering", "2022-11-01", "2024-07-01"),
        // Title worded differently, start month differs from the site (2020-01).
        exp("Connecticut College", "Computational Biology and Informatics Researcher", "2020-02-01", "2021-05-01"),
        // Decoy: contains every word of the site's title; must not be chosen.
        exp(
            "Connecticut College",
            "Summer Science Research Institute Bioinformatics and Computational Biology Researcher",
            "2020-05-01",
            "2020-06-01",
        ),
    ];
    let f: Vec<String> = compare(&p, &site, date!(2026 - 10 - 06)).iter().map(ToString::to_string).collect();
    let has = |prefix: &str, text: &str| f.iter().any(|x| x.starts_with(prefix) && x.contains(text));
    assert!(has("VIOLATION", "valid until 2027-03-31, the website says expires 2028-03"), "{f:#?}");
    assert!(!has("VIOLATION", "Certified Nursing Assistant"), "{f:#?}");
    assert!(has("note", "\"Wilderness First Responder\" is not on the website"), "{f:#?}");
    assert!(has("PENDING", "lists \"Basic Life Support (BLS)\""), "{f:#?}");
    assert!(has("note", "titled \"Computational Biology & Bioinformatics Researcher\""), "{f:#?}");
    assert!(has("VIOLATION", "start 2020-02 in experiences.toml, 2020-01 on the website"), "{f:#?}");
    assert!(has("PENDING", "lists \"Aerospace Medicine, Space Operations\" at SpaceX"), "{f:#?}");
    assert!(!f.iter().any(|x| x.contains("Director of Information Technology")), "exact match, same dates: {f:#?}");
    assert_eq!(f.iter().filter(|x| x.starts_with("VIOLATION")).count(), 2, "{f:#?}");
}

#[test]
fn an_expired_published_license_without_a_profile_expiry_is_a_violation() {
    let mut p = Profile::default();
    p.licenses.licenses.push(license("BLS", "Board", None, "ID"));
    let site = admit_website::Published {
        roles: vec![],
        credentials: vec![admit_website::Credential {
            name: "BLS".into(),
            organization: "Board".into(),
            identifier: Some("ID".into()),
            expires: Some((2020, 1)),
        }],
    };
    let f = compare(&p, &site, date!(2026 - 10 - 06));
    assert_eq!(f.iter().filter(|f| f.is_violation()).count(), 2);
    assert!(f.iter().any(|f| f.to_string().contains("no valid_until")));
    assert!(f.iter().any(|f| f.to_string().contains("expired 2020-01")));
}

#[test]
fn parser_edges_skip_broken_markup_and_unmatched_titles() {
    assert_eq!(expiry("expires "), None);
    assert_eq!(expiry("expires !!!"), None);
    assert_eq!(expiry("expires xyz 2028"), None);
    assert_eq!(expiry("expires sep"), None);
    assert_eq!(expiry("expires sep nope"), None);

    assert!(parse("application/ld+json").roles.is_empty());
    assert!(parse("application/ld+json>{\"@type\":\"Person\"}").credentials.is_empty());
    assert!(parse(r#"<script type="application/ld+json">{not json}</script>"#).roles.is_empty());

    let html = r#"
        <script type="application/ld+json">
        {"@type":"Person","hasCredential":[{"recognizedBy":{"name":"Board"}}]}
        </script>
        <script type="application/ld+json">
        {"@type":"ItemList","name":"Professional Experience","itemListElement":[
            {"item":{"worksFor":{"name":"Clinic"}}},
            {"item":{"roleName":"Nurse","worksFor":{"name":"Clinic"}}}
        ]}
        </script>
    "#;
    let p = parse(html);
    assert!(p.credentials.is_empty());
    assert_eq!(p.roles.len(), 1);
    let page = published("https://cameronaaron.com", html).expect("page");
    assert_eq!(page.roles.len(), 1);

    let mut profile = Profile::default();
    profile.experiences.experiences.push(exp("Clinic", "RN", "2020-01-01", "2021-01-01"));
    let short = admit_website::Published {
        roles: vec![admit_website::Role { title: "IT".into(), organization: "Clinic".into(), start: None, end: None }],
        credentials: vec![],
    };
    let pending = compare(&profile, &short, date!(2026 - 10 - 06));
    assert!(pending.iter().any(|f| f.to_string().contains("lists \"IT\"")), "{pending:?}");

    let mut dated = Profile::default();
    dated.experiences.experiences.push(exp("Clinic", "Nurse", "not-a-date", "also-bad"));
    let bad = admit_website::Published {
        roles: vec![admit_website::Role {
            title: "Nurse".into(),
            organization: "Clinic".into(),
            start: Some("nope".into()),
            end: Some("still-nope".into()),
        }],
        credentials: vec![],
    };
    assert!(compare(&dated, &bad, date!(2026 - 10 - 06)).is_empty());
}

#[tokio::test]
async fn fetch_reads_a_page_and_reports_failures() {
    tokio::task::unconstrained(async move {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ok"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>ok</html>"))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/down")).respond_with(ResponseTemplate::new(503)).mount(&server).await;
        let html = fetch(&format!("{}/ok", server.uri())).await.expect("200");
        assert!(html.contains("ok"), "{html}");

        let err = fetch(&format!("{}/down", server.uri())).await.expect_err("503");
        let SiteError::Http { status: 503, .. } = err else { panic!("{err}") };

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let truncated = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            use std::io::Write;
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n");
        });
        let err = fetch(&format!("http://{truncated}/")).await.expect_err("truncated");
        assert!(matches!(err, SiteError::Fetch { .. }), "{err}");

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);
        let err = fetch(&format!("http://{addr}/")).await.expect_err("refused");
        assert!(matches!(err, SiteError::Fetch { .. }), "{err}");
        assert!(err.to_string().contains(&addr.to_string()), "{err}");
    })
    .await
}
