use wiremock::matchers::{body_string_contains, header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
const PRINCIPAL: &str = include_str!("../fixtures/ps_principal_request.html");
const COMPONENT: &str = "/psc/csprd_5/EMPLOYEE/SA/c/SSR_STUDENT_ACAD_REC_FL.SSR_VW_UNOFF_TS_FL.GBL";

pub async fn portal(pdf: Vec<u8>) -> MockServer {
    let s = MockServer::start().await;
    let uri = s.uri();
    let signin = "/adfs/ls/idpinitiatedsignon.aspx";
    let login_form = format!(
        "<form method=\"post\" id=\"loginForm\" action=\"{signin}?loginToRp=csprd.laccd.edu&amp;client-request-id=1\">\
         <input name=\"UserName\" value=\"\"><input name=\"Password\" type=\"password\"></form>"
    );
    Mock::given(method("GET"))
        .and(path(signin))
        .and(header_regex("cookie", "MSISAuth=ok"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            "<form method=\"POST\" name=\"hiddenform\" action=\"{uri}/saml2/sp/acs/post\">\
             <input type=\"hidden\" name=\"SAMLResponse\" value=\"PHNhbWw+\" /></form>"
        )))
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path(signin))
        .respond_with(ResponseTemplate::new(200).set_body_string(login_form.clone()))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path(signin))
        .and(body_string_contains("Password=right+pass"))
        .and(body_string_contains("AuthMethod=FormsAuthentication"))
        .and(body_string_contains("UserName=escsso%5C123"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", format!("{signin}?loginToRp=csprd.laccd.edu"))
                .insert_header("Set-Cookie", "MSISAuth=ok; path=/"),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path(signin))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            "{login_form}<span id=\"errorText\" role=\"alert\">Incorrect user ID or password.</span>"
        )))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/saml2/sp/acs/post"))
        .and(body_string_contains("SAMLResponse=PHNhbWw%2B"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "/psoftsso/PrincipalRequest")
                .insert_header("Set-Cookie", "JSESSIONID=j1; path=/"),
        )
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/psoftsso/PrincipalRequest"))
        .and(header_regex("cookie", "JSESSIONID=j1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(PRINCIPAL))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/psp/csprd/"))
        .and(body_string_contains("sso_ticket=%3Credacted%3E"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("Set-Cookie", "PS_TOKEN=t; path=/").set_body_string("home"),
        )
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path(COMPONENT))
        .and(header_regex("cookie", "PS_TOKEN=t"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "<input type='hidden' name='ICSID' id='ICSID' value='S1/x=' /><input id='ICStateNum' value='5'/>",
        ))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path(COMPONENT))
        .and(body_string_contains("ICStateNum=5&"))
        .and(body_string_contains("ICSID=S1%2Fx%3D"))
        .and(body_string_contains("ICAction=DERIVED_SSTSRPT_REPORT_REQUEST_PB"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            "<GENSCRIPT><![CDATA[window.open('{uri}/psc/csprd_5/view/%7bV2%7dabc/LAC_SR_TSRPT.pdf','','');]]></GENSCRIPT>"
        )))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/psc/csprd_5/view/%7bV2%7dabc/LAC_SR_TSRPT.pdf"))
        .respond_with(ResponseTemplate::new(200).insert_header("Content-Type", "application/pdf").set_body_bytes(pdf))
        .mount(&s)
        .await;
    s
}
