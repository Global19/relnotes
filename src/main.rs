extern crate reqwest;
extern crate chrono;
extern crate serde;
extern crate serde_json as json;
#[macro_use] extern crate askama;

type JsonRefArray<'a> = Vec<&'a json::Value>;

use std::collections::BTreeMap;
use std::env;

use askama::Template;
use chrono::prelude::*;
use chrono::Duration;

#[derive(Template)]
#[template(path = "relnotes.md")]
struct ReleaseNotes {
    version: String,
    date: NaiveDate,
    language_relnotes: String,
    language_unsorted: String,
    libraries_relnotes: String,
    libraries_unsorted: String,
    compiler_relnotes: String,
    compiler_unsorted: String,
    cargo_relnotes: String,
    cargo_unsorted: String,
    unsorted_relnotes: String,
    unsorted: String,
    links: String,
    cargo_links: String,
}

fn main() {
    let mut args = env::args();
    let _ = args.next();
    let version = args.next().unwrap();
    let today = Utc::now().date();

    // A known rust release date.
    let mut end = Utc.ymd(2017, 7, 20);
    let six_weeks = Duration::weeks(6);

    // Get the most recent rust release date.
    while today - end > six_weeks { end = end + six_weeks; }

    let start = end - six_weeks;
    let issues = get_issues(start, end, "rust");

    // Skips `beta-accepted` as those PRs were backported onto the
    // previous stable.
    let in_release = issues.iter().filter(|v| {
        !v["labels"]["nodes"].as_array()
                             .unwrap()
                             .iter()
                             .any(|o| o["name"] == "beta-accepted" ||
                                      o["name"] == "T-doc")
    });

    let links = map_to_link_items("", in_release.clone());
    let (relnotes, rest) = partition_by_tag(in_release, "relnotes");

    let (libraries_relnotes,
         language_relnotes,
         compiler_relnotes,
         unsorted_relnotes) = partition_prs(relnotes);

    let (libraries_unsorted,
         language_unsorted,
         compiler_unsorted,
         unsorted) = partition_prs(rest);

    let cargo_issues = get_issues(start, end, "cargo");
    let (cargo_relnotes, cargo_unsorted) = {
        let (relnotes, rest) = partition_by_tag(cargo_issues.iter(), "relnotes");

        (
            map_to_line_items("cargo/", relnotes),
            map_to_line_items("cargo/", rest)
        )
    };

    let cargo_links = map_to_link_items("cargo/", cargo_issues.iter());

    let relnotes = ReleaseNotes {
        version,
        date: (end + six_weeks).naive_utc(),
        language_relnotes,
        language_unsorted,
        libraries_relnotes,
        libraries_unsorted,
        compiler_relnotes,
        compiler_unsorted,
        cargo_relnotes,
        cargo_unsorted,
        unsorted_relnotes,
        unsorted,
        links,
        cargo_links,
    };

    println!("{}", relnotes.render().unwrap());
}

fn get_issues(start: Date<Utc>, end: Date<Utc>, repo_name: &'static str)
    -> Vec<json::Value>
{
    use std::env;

    use reqwest::{
        Client,
        header::*,
        mime,
    };

    let mut headers = Headers::new();
    headers.set(ContentType(mime::APPLICATION_JSON));
    headers.set(Accept(vec![QualityItem::new(mime::APPLICATION_JSON, Quality::default())]));
    headers.set(Authorization(Bearer { token: env::var("GITHUB_API_KEY").unwrap() }));
    headers.set(UserAgent::new("Rsearcher/0.1.0"));
    let mut args = BTreeMap::new();
    args.insert("states", String::from("[MERGED]"));
    args.insert("last", String::from("100"));
    let mut issues = Vec::new();
    let mut not_found_window = true;

    loop {
        let query = format!("
            query {{
                repository(owner: \"rust-lang\", name: \"{repo_name}\") {{
                    pullRequests({args}) {{
                        nodes {{
                            mergedAt
                            number
                            title
                            url
                            labels(last: 100) {{
                                nodes {{
                                    name
                                }}
                            }}
                        }}
                        pageInfo {{
                            startCursor
                        }}
                    }}
                }}
            }}",
            repo_name = repo_name,
            args = args.iter()
                       .map(|(k, v)| format!("{}: {}", k, v))
                       .collect::<Vec<_>>()
                       .join(",")
        ).replace(" ", "").replace("\n", " ").replace('"', "\\\"");


        let json_query = format!("{{\"query\": \"{}\"}}", query);

        let client = Client::new();

        let mut response = client.post("https://api.github.com/graphql")
            .headers(headers.clone())
            .body(json_query)
            .send()
            .unwrap();

        let json: json::Value = response.json().unwrap();

        let pull_requests_data = json["data"]["repository"]["pullRequests"].clone();

        let mut pull_requests = pull_requests_data["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| {
                let merge_date: chrono::Date<_> = o["mergedAt"]
                    .as_str()
                    .unwrap()
                    .parse::<DateTime<_>>()
                    .unwrap()
                    .date();

                (merge_date < end) && (merge_date > start)
            })
            .cloned()
            .collect::<Vec<_>>();

        args.insert(
            "before",
            format!("\"{}\"",
                pull_requests_data["pageInfo"]["startCursor"].clone()
                    .as_str()
                    .unwrap()
                    .to_owned()
            )
        );

        if pull_requests.len() != 0 {
            not_found_window = false;
            issues.append(&mut pull_requests);
        } else if not_found_window {
            continue
        } else {
            break issues
        }

    }
}

fn map_to_line_items<'a>(prefix: &'static str,
                         iter: impl IntoIterator<Item=&'a json::Value>)
    -> String
{
    iter.into_iter().map(|o| {
        format!(
            "- [{prefix}{title}][{number}]",
            prefix = prefix,
            title = o["title"].as_str().unwrap(),
            number = o["number"],
        )
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn map_to_link_items<'a>(prefix: &'static str,
                         iter: impl IntoIterator<Item=&'a json::Value>)
    -> String
{
    iter.into_iter().map(|o| {
            format!(
                "[{prefix}{number}]: {url}/",
                prefix = prefix,
                number = o["number"],
                url = o["url"].as_str().unwrap()
            )
        })
    .collect::<Vec<_>>()
    .join("\n")
}

fn partition_by_tag<'a>(iter: impl IntoIterator<Item=&'a json::Value>,
                        tag: &str)
    -> (JsonRefArray<'a>, JsonRefArray<'a>)
{
    iter.into_iter().partition(|o| {
        o["labels"]["nodes"].as_array()
                            .unwrap()
                            .iter()
                            .any(|o| o["name"] == tag)
    })
}

fn partition_prs<'a>(iter: impl IntoIterator<Item=&'a json::Value>)
    -> (String, String, String, String)
{
    let (libs, rest) = partition_by_tag(iter, "T-libs");
    let (lang, rest) = partition_by_tag(rest, "T-lang");
    let (compiler, rest) = partition_by_tag(rest, "T-compiler");

    (
        map_to_line_items("", libs),
        map_to_line_items("", lang),
        map_to_line_items("", compiler),
        map_to_line_items("", rest)
     )
}
