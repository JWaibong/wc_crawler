use std::{fs, time::Duration};

use image::{DynamicImage, ImageError};
use reqwest::{Client, StatusCode};
use scraper::{Html, Selector};
use tokio::task::{spawn_blocking, JoinError, JoinSet};
use url::Url;


static MANGA_TITLE: &'static str = "VAGABOND";

static IMG_API_HOST: &'static str = "cdn.readkakegurui.com";

static SELECTOR: &'static str = "img";

static ATTR: &'static str = "data-src";

static NUM_CHAPTERS: usize = 328;

const URLS: &'static [&'static str; 2] = &["https://w65.readvagabond.com/manga/vagabond-chapter-326-samurai/","https://w65.readvagabond.com/manga/vagabond-chapter-327-the-man-named-tadaoki/"];
#[tokio::main]
async fn main() {

    tokio::spawn(async {

        let mut chapters_join_set = JoinSet::new();
        // for i in 1..=NUM_CHAPTERS {
        //     chapters_join_set.spawn(async move {
        //         // let url = format!("https://www.firepunchmangafree.com/manga/firepunch-chapter-{}/index.html", i);
        //         // let url = format!("https://ww2.jujustukaisen.com/manga/jujutsu-kaisen-chapter-{}/", i);
        //         let url = format!("https://w65.readvagabond.com/manga/vagabond-chapter-{}-shinmen-takezo/", i);
        //         println!("Start download for chapter {}", i);
        //         download_chapter(i, url).await
        //     });
        //     tokio::time::sleep(Duration::from_secs(1)).await;
        // }

        for (i, url) in URLS.into_iter().enumerate() {
            chapters_join_set.spawn(async move {
                println!("Start download for chapter {}", i+326);
                download_chapter(i+326, url.to_string()).await
            });
            tokio::time::sleep(Duration::from_secs(1)).await;

        }

        let mut chapters: Vec<(usize, Vec<(usize, DynamicImage)>)> = Vec::new();
        while let Some(res) = chapters_join_set.join_next().await {
            if res.is_err() {
                eprintln!("{:?}", res.unwrap_err());
                continue;
            }

            let res = res.unwrap();
            if res.is_none() {
                eprintln!("Unable to obtain all images for a chapter");
                continue;
            }
            chapters.push(res.unwrap());
        }

        if fs::metadata(MANGA_TITLE).is_err() {
            tokio::fs::create_dir(MANGA_TITLE).await.unwrap();
        }
        for (chapter_num, page) in chapters.into_iter() {
            let chapter_path = format!("{}/chapter_{}", MANGA_TITLE, chapter_num);
            tokio::fs::create_dir(chapter_path.clone()).await.unwrap();

            for (img_num, img) in page.into_iter(){
                let res = img.save(format!("{}/img_{}.jpg", chapter_path, img_num));
                if res.is_err() {
                    eprintln!("{:?}", res.unwrap_err())
                }
            }
            println!("Successfully downloaded chapter {}", chapter_num)
        }
    }).await.unwrap();

}


#[derive(Debug)]
enum DownloadImageError {
    ReqwestError(reqwest::Error),
    ResponseStatusError(StatusCode),
    LoadImageError(JoinError),
    SaveImageError(ImageError),
}

impl From<reqwest::Error> for DownloadImageError {
    fn from(err: reqwest::Error) -> Self {
        Self::ReqwestError(err)
    }
}

impl From<JoinError> for DownloadImageError {
    fn from(err: JoinError) -> Self {
        Self::LoadImageError(err)
    }
}

impl From<ImageError> for DownloadImageError {
    fn from(err: ImageError) -> Self {
        Self::SaveImageError(err)
    }
}

async fn download_chapter(chapter_num: usize, url: String) -> Option<(usize, Vec<(usize, DynamicImage)>)> {

    let html = get_html(url.as_str()).await;
    if let Some(html) = html {
        let image_urls = spawn_blocking(move || {
            parse_img_srcs (url, html)
        }).await.unwrap();

        let imgs = download_images(image_urls).await;
        if imgs.is_none() {
            return None;
        }

        return Some((chapter_num, imgs.unwrap()));
    }
    None
}

async fn download_image(url: &str) -> Result<Option<DynamicImage>, DownloadImageError> {
    let client = Client::new();
    let request = client.get(url).build()?;

    let mut response = client.execute(request.try_clone().unwrap()).await;

    loop {
        if response.is_err() {
            eprintln!("Could not execute request for image at url {}, most likely due to rate limit. Trying again", url);
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            response = client.execute(request.try_clone().unwrap()).await;
        }
        else {
            break; 
        }
    }
    let response = response.unwrap();
    if response.status().is_success() {

        let data = response.bytes().await?.to_vec();

        let image_result = spawn_blocking(move || {
            image::load_from_memory(data.as_slice()).ok()
        }).await?;

        return Ok(image_result);
    }
    
    Err(DownloadImageError::ResponseStatusError(response.status()))

}

async fn get_html(url: &str) -> Option<String> {
    let client = Client::new();
    let request = client.get(url).build().ok();
    if request.is_none() {
        return None;
    }
    let response = client.execute(request.unwrap()).await;
    let text = response.ok().map(|res| { res.text() });
    if text.is_none() {
        return None;
    }
    text.unwrap().await.ok()
}


fn parse_img_srcs(content_url: String, html: String) -> Vec<String> {
    let document = Html::parse_fragment(html.as_str());
    let img_selector = Selector::parse(SELECTOR).unwrap();
    let mut image_urls: Vec<String> = Vec::new();

    for element in document.select(&img_selector) {
        if let Some(url) = element.attr(ATTR) {
            let parsed_url = Url::parse(url);
            if parsed_url.is_err() {
                let mut splt = url.split_terminator("../");
                let count = url.split_terminator("../").count(); 
                let content_url_split: Vec<_> = content_url.split_terminator("/").collect();
                let mut it = content_url_split.into_iter().rev();
                for _ in 0..count-1{
                    splt.next();
                    it.next();
                }
                it.next();
                let base_url: Vec<_> = it.rev().collect();
                let relative_url: Vec<_> = splt.collect();
                let mut resolved = base_url.join("/");
                resolved.push_str("/");
                resolved.push_str(relative_url.join("").as_str());
                
                let parsed = Url::parse(resolved.as_str());
                if parsed.is_ok() {
                    let parsed = parsed.unwrap();
                    if parsed.has_host() && parsed.host_str().unwrap().eq(IMG_API_HOST) {
                        println!("Resolved url for image with src: {}", resolved.as_str());
                        image_urls.push(resolved);
                    }
                }
            }
            else {
                let parsed = parsed_url.unwrap();
                if parsed.scheme().ne("https") {
                    // println!("Found a url without https scheme: {}", url);
                    continue;
                }
                else if parsed.host_str().unwrap().ne(IMG_API_HOST) {
                    // println!("Found a url without host {}: {}", IMG_API_HOST, url);
                    continue;
                }
                else {
                    image_urls.push(url.to_string());
                }
            }
        }
    }
    // println!("{:#?}", image_urls);
    image_urls
}


async fn download_images(image_urls: Vec<String>) -> Option<Vec<(usize, DynamicImage)>> {

    let mut imgs: Vec<(usize, DynamicImage)> = Vec::new();
    for (i, src) in image_urls.into_iter().enumerate().into_iter() {
            let result = download_image(
                src.as_str()
            ).await;
            
            if result.is_err() {
                eprintln!("No image found for url {} {} {:?}", i, src, result.unwrap_err());
                continue;
            }
            let result = result.unwrap();
            if result.is_none() {
                eprintln!("No image found for {} {}", i, src);
                continue;
            }
            imgs.push((i, result.unwrap()));
    }
    Some(imgs)
}

async fn download_images_with_join_set(image_urls: Vec<String>) -> Option<Vec<(usize, DynamicImage)>> {
    let mut img_join_set = JoinSet::new();
    for (i, src) in image_urls.into_iter().enumerate().into_iter() {
        img_join_set.spawn(async move {
            let result = download_image(
                src.as_str()
            ).await;
            
            if result.is_err() {
                eprintln!("No image found for url {} {:?}", src, result.unwrap_err());
                return None;
            }
            let result = result.unwrap();
            if result.is_none() {
                eprintln!("No image found for {} {}", i, src);
                return None;
            }
            return Some((i, result.unwrap()));
        });
    }

    let mut imgs: Vec<(usize, DynamicImage)> = Vec::new();
    while let Some(res) = img_join_set.join_next().await {
        if res.is_err() {
            eprintln!("No image found {:?}", res.unwrap_err());
            continue;
        }
        let img = res.unwrap(); 
        if img.is_none() {
            eprintln!("No image found?");
            continue;
        }
        imgs.push(img.unwrap());
    }
    Some(imgs)
}

#[test]
fn foo() {
    use std::cmp::Ordering;

    let content_url = "https://www.firepunchmangafree.com/manga/firepunch-chapter-1/index.html";
    let x = "../../sun9-68.userapi.com/impg/9S4KGO552So7NlP0BWEBLk2fzkU4oE8BP1XwTg/m6_h-tJ8CEU9125.jpg?size=827x1300&quality=95&sign=dffb83ea574efeef211aa2d1898dc9d5&type=album";
    let parsed_url = Url::parse(x);
    assert!(parsed_url.is_err());
    let mut splt = x.split_terminator("../");
    let count = x.split_terminator("../").count(); 
    let content_url_split: Vec<_> = content_url.split_terminator("/").collect();
    let mut it = content_url_split.into_iter().rev();
    for _ in 0..count-1{
        splt.next();
        it.next();
    }
    it.next();
    let base_url: Vec<_> = it.rev().collect();
    let relative_url: Vec<_> = splt.collect();
    let mut resolved = base_url.join("/");
    resolved.push_str("/");
    resolved.push_str(relative_url.join("").as_str());
    let expected = String::from("https://www.firepunchmangafree.com/sun9-68.userapi.com/impg/9S4KGO552So7NlP0BWEBLk2fzkU4oE8BP1XwTg/m6_h-tJ8CEU9125.jpg?size=827x1300&quality=95&sign=dffb83ea574efeef211aa2d1898dc9d5&type=album");
    assert_eq!(String::cmp(&resolved, &expected), Ordering::Equal);
    println!("{}", Url::parse(expected.as_str()).unwrap().scheme())
}

#[test]
fn foo2() {
    let url = "data:image/svg+xml,%3Csvg%20xmlns=%22http://www.w3.org/2000/svg%22%20viewBox=%220%200%20340%20484%22%3E%3C/svg%3E";
    let res = Url::parse(url);
    assert!(res.is_ok());
    println!("{}", res.unwrap().scheme());
}

#[test]
fn foo4() {

    let document = Html::parse_fragment("<img src=\"data:image/svg+xml,%3Csvg%20xmlns=%22http://www.w3.org/2000/svg%22%20viewBox=%220%200%20210%20140%22%3E%3C/svg%3E\" data-src=\"https://cdn.readkakegurui.com/file/mangaifenzi22/vagabond/vol-37-chapter-326-to-be-a-samurai/23.jpg\">");
    let img_selector = Selector::parse("img").unwrap();

    for element in document.select(&img_selector) {
        println!("{:#?}", element.attr("data-src").unwrap());
    }
}


#[tokio::test]
async fn test_download_img() {
    let url = String::from("https://www.firepunchmangafree.com/sun9-68.userapi.com/impg/9S4KGO552So7NlP0BWEBLk2fzkU4oE8BP1XwTg/m6_h-tJ8CEU9125.jpg?size=827x1300&quality=95&sign=dffb83ea574efeef211aa2d1898dc9d5&type=album");

    let res = download_image(url.as_str()).await;
    assert!(res.is_ok());
    let res = res.unwrap();
    assert!(res.is_some());
}


#[test]
fn foo3() {
    //let url = "https://1.bp.blogspot.com/-0I-TV0X8LK4/XgipoNuX3jI/AAAAAAAADIA/bFIwes9EBKc4H8RzXvWF1QFem8IsUgREwCLcBGAsYHQ/s1600/003.jpg";
    let url = "https://w65.readvagabond.com/manga/#content";
    let res = Url::parse(url);
    assert!(res.is_ok());
    println!("{}", res.unwrap().host_str().unwrap());
}