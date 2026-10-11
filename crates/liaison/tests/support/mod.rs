use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub async fn get(server: &MockServer, endpoint: &str, value: Value) {
    Mock::given(method("GET"))
        .and(path(format!("/applicant/api{endpoint}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(value))
        .mount(server)
        .await;
}

pub async fn portal() -> MockServer {
    let s = MockServer::start().await;
    let rows = json!([
        {"id":222,"text":"United States"},{"id":333,"text":"Canada"},
        {"id":339,"text":"California"},{"id":1,"text":"Awards"},{"id":2,"text":"Honors"},
        {"id":3,"text":"Employment"},{"id":4,"text":"Community Enrichment"},
        {"id":5,"text":"Patient/Healthcare Experience"},{"id":6,"text":"Part time"},
        {"id":7,"text":"Certification"},{"id":8,"text":"Licensure"},
        {"id":9,"text":"Bachelor of Arts"},{"id":10,"text":"History"},
        {"id":11,"text":"Art"},{"id":12,"text":"Music"},
        {"id":1,"text":"January"},{"id":8,"text":"May"},{"id":9,"text":"June"},
        {"id":12,"text":"September"},{"id":13,"text":"October"},
        {"id":2013,"text":"2013"},{"id":2017,"text":"2017"},{"id":2020,"text":"2020"},
        {"id":2026,"text":"2026"},{"id":2027,"text":"2027"},
        {"id":471,"text":"Spring"},{"id":473,"text":"Fall"},
        {"id":466,"text":"Semester"},{"id":1,"text":"Freshman"},{"id":1,"text":"Other Science"}
    ]);
    for p in [
        "countries",
        "states",
        "achievementTypes",
        "experienceTypes",
        "employmentStatusTypes",
        "license",
        "degreeType",
        "collegeMinor",
        "month",
        "year",
        "terms",
        "termType",
        "academicStatus",
        "subjects",
    ] {
        get(&s, &format!("/lookup/{p}"), rows.clone()).await;
    }
    get(&s,"/supportingInfo/achievements",json!({"achievements":[{"id":1,"achievement":"Awards","name":"Award","organization":"Org","issuedDate":"2020-01-01","stateId":339,"countryId":222,"description":"Awarded"},{"id":2,"achievement":"Honors","name":"Honor","stateId":339,"countryId":222}]})).await;
    get(&s,"/supportingInfo/licenses/blockName/license2",json!({"licenses":[{"id":3,"typeName":"Certification","title":"License","organization":"Board"},{"id":4,"typeName":"Licensure","title":"Registration","organization":"Board","description":"Registered","validUntilDateString":"2027-01-01"}]})).await;
    get(&s, "/supportingInfo/experiences", json!({"experiences":[{"id":5},{"id":6},{"id":7}]})).await;
    for (id, kind, current) in [(5, 3, false), (6, 4, true), (7, 5, false)] {
        get(&s,&format!("/questionBlock/supportingInfo/experiences/{id}"),json!({"questionBlocks":[
            {"id":"experienceType","data":{"experienceTypeId":kind}},
            {"id":"organization","data":{"name":"Clinic","stateId":339,"countryId":222}},
            {"id":"employmentDates","data":{"startDate":"2020-01-01","endDate":"2020-02-01","currentlyEmployed":current,"statusId":6}},
            {"id":"employmentDetails","data":{"jobTitle":format!("Role {id}"),"averageWeeklyHours":10,"numberOfWeeks":4,"totalHours":40,"compensated":true,"volunteer":false,"jobDescription":"Work"}},
            {"id":"unused","data":null},{"id":"experienceDomain","data":{"domains":[1]}}
        ]})).await;
    }
    get(&s,"/academicHistory/colleges",json!({"collegesList":[{"id":8,"college":"School","termTypeId":466,"stillAttending":false,"degrees":[
        {"id":10,"degreeStatusId":431,"type":"Bachelor of Arts","typeId":9,"major":"History","majorId":10,"month":"May","monthId":8,"year":"2020","yearId":2020},
        {"id":11,"degreeStatusId":430,"type":"Bachelor of Arts","typeId":9,"major":"Art","majorId":11,"month":"May","monthId":8,"year":"2027","yearId":2027,"secondMajor":"Music","secondMajorId":12,"minor":"History","minorId":10}
    ]}]})).await;
    get(&s,"/academicHistory/highSchools",json!({"data":[{"id":9,"highSchoolName":"HS","fromYear":"2013","fromMonth":"September","toYear":"2017","toMonth":"May","termType":"Semester","termTypeId":466}]})).await;
    get(
        &s,
        "/navigation",
        json!({"sections":[{"id":42,"name":"highSchoolAttended"}],
        "personalInfo":{"sections":[{"id":24164,"name":"biographicInfo"},{"id":24166,"name":"contactInfo"}]}}),
    )
    .await;
    // Biographic and Contact Information, trimmed from the 2026-10-06 capture.
    get(
        &s,
        "/applicationFormSubSectionBlocks/subSection/24164",
        json!({"questionBlocks":[{"id":"name","questions":[
        {"shortFieldName":"first_name","value":["Pat"],"answerOptions":[]},
        {"shortFieldName":"last_name","value":["Doe"],"answerOptions":[]}]}]}),
    )
    .await;
    get(&s, "/applicationFormSubSectionBlocks/subSection/24166", json!({"questionBlocks":[]})).await;
    get(&s, "/authenticatedUser", json!({"email":"pat@example.com"})).await;
    get(&s,"/academicHistory/highSchoolInfo/42",json!({"questionBlocks":[{"id":"graduationStatus","data":{"diplomaOrEquivalency":"Diploma","diplomaOrEquivalencyDate":"2017-05","highSchoolName":"HS"}},{"id":"other","data":{"keep":true}}]})).await;
    get(&s, "/academicHistory/courseWork", json!([])).await;
    get(&s, "/getProgramNavigation", json!([{"orgName":"University","programName":"Nursing","programSelectedId":1}]))
        .await;
    get(&s, "/programsSelected", json!({"selectedPrograms":[]})).await;
    get(
        &s,
        "/programMaterials/evaluations/programSelected/1",
        json!({"evaluationTypes":[{"minRequired":1}],"evaluationsRequested":0,"evaluationsCompleted":0}),
    )
    .await;
    get(&s, "/transcripts", json!({"college":{"collegeMap":[]}})).await;
    get(&s, "/academicHistory/courseWork/prerequisites", json!([])).await;
    get(&s, "/programMaterials/prerequisites/transcripts", json!({"data": {"transcripts": []}})).await;
    Mock::given(method("POST"))
        .and(path("/applicant/api/search/filteredQueries"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"programs":[]})))
        .mount(&s)
        .await;
    for verb in ["POST", "PUT"] {
        Mock::given(method(verb))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":99})))
            .mount(&s)
            .await;
    }
    s
}
