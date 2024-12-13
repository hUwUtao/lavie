use std::io::Write;

use anyhow::{Context, Error, Ok, Result};
use font_kit::family_name::FamilyName;
use font_kit::properties::Properties;
use font_kit::source::SystemSource;
use image::{DynamicImage, Rgba};
use jpeg_encoder::{ColorType, Encoder};
use raqote::{
    AntialiasMode, Color, DrawOptions, DrawTarget, GradientStop, Image, Point, SolidSource, Source,
};
use rayon::prelude::*;
use reqwest::Client;
use tokio::fs::File;

#[derive(serde::Deserialize)]
struct BlogPost {
    title: String,
    thumb: Option<Thumbnail>,
}

#[derive(serde::Deserialize)]
struct Thumbnail {
    url: String,
    rendition: Option<Rendition>,
}

#[derive(serde::Deserialize)]
struct Rendition {
    url: String,
}

fn u8rgba_u32argb(img: &image::ImageBuffer<Rgba<u8>, Vec<u8>>) -> Vec<u32> {
    let mut target = Vec::with_capacity(img.len());
    img.par_chunks(4)
        .map(|chunk| {
            let r = chunk[0] as u32;
            let g = chunk[1] as u32;
            let b = chunk[2] as u32;
            let a = chunk[3] as u32;
            (a << 24) | (r << 16) | (g << 8) | b
        })
        .collect_into_vec(&mut target);
    target
}

async fn fetch_blog_post(client: &Client, slug: &str) -> Result<BlogPost> {
    println!("Fetching");
    let url = format!("http://localhost:4321/api/blog?slug={}", slug);

    let response = client
        .get(&url)
        .send()
        .await?
        .json::<BlogPost>()
        .await
        .context("Failed to parse blog post")?;

    Ok(response)
}

async fn load_image(client: &Client, url: &str) -> Result<DynamicImage> {
    println!("Loading texture {}", url);
    let response = client
        .get(url)
        .send()
        .await?
        .bytes()
        .await
        .context("Failed to download image bytes")?;

    let img = image::load_from_memory(&response).context("Failed to load image from memory")?;

    Ok(img)
}

async fn render_thumbnail(w: Box<dyn Write>, client: &Client, blog: &BlogPost) -> Result<()> {
    const WIDTH: i32 = 1200;
    const HEIGHT: i32 = 630;

    let mut dt = DrawTarget::new(WIDTH, HEIGHT);
    let mut draw_o = DrawOptions::new();
    draw_o.antialias = AntialiasMode::Gray;
    draw_o.alpha = 1.0;

    // Background (light gray)
    dt.fill_rect(
        0.0,
        0.0,
        WIDTH as f32,
        HEIGHT as f32,
        &Source::Solid(SolidSource {
            r: 248,
            g: 249,
            b: 250,
            a: 255,
        }),
        &draw_o,
    );

    // Load and draw thumbnail image
    if let Some(thumb) = &blog.thumb {
        // let thumb_image = load_image(client, &thumb.url).await?;
        // let resized_image = thumb_image.resize_to_fill(
        //     WIDTH as u32,
        //     HEIGHT as u32,
        //     image::imageops::FilterType::Lanczos3,
        // );

        // Convert to RGBA and draw
        // dt.draw_image_with_size_at(0.0, 0.0, WIDTH, HEIGHT, &resized_image.to_rgba8());

        // Alternative rendition handling

        if let Some(rendition) = &thumb.rendition {
            let rendition_image = load_image(client, &rendition.url).await?;
            let rem = u8rgba_u32argb(&rendition_image.to_rgba8());
            dt.draw_image_at(
                0.0,
                0.0,
                &Image {
                    width: rendition_image.width() as i32,
                    height: rendition_image.height() as i32,
                    data: &rem,
                },
                &draw_o,
            );
        }
    }

    // Optional: Add darkening gradient overlay
    // let gradient = LinearGradient::new(0.0, 0.0, WIDTH as f32, HEIGHT as f32);
    let gradient = raqote::Gradient {
        stops: vec![
            GradientStop {
                position: 0.0,
                color: Color::new(128, 255, 0, 0),
            },
            GradientStop {
                position: 1.0,
                color: Color::new(128, 0, 255, 0),
            },
        ],
    };

    dt.fill_rect(
        0.0,
        0.0,
        WIDTH as f32,
        HEIGHT as f32,
        &Source::new_linear_gradient(
            gradient,
            Point::zero(),
            Point::new(1200.0, 630.0),
            raqote::Spread::Repeat,
        ),
        &draw_o,
    );
    dt.draw_text(
        &SystemSource::new()
            .select_best_match(&[FamilyName::SansSerif], &Properties::new())
            .unwrap()
            .load()
            .unwrap(),
        120.0,
        "Ligma",
        Point::new(256.0, 256.0),
        &Source::Solid(SolidSource::from_unpremultiplied_argb(255, 255, 0, 255)),
        &draw_o,
    );

    println!("rendering");
    // dt.write_png("./thumb.png")?;
    render_chunked(w, dt, 75)?;
    println!("done");

    Ok(())
}

fn render_chunked(w: Box<dyn Write>, dt: DrawTarget, quality: u8) -> Result<(), Error> {
    let (width, height) = (dt.width() as usize, dt.height() as usize);

    // Prepare a vector for RGB data
    // let mut rgb_data: Vec<u8> = Vec::with_capacity(width * height * 3);

    // Process pixels in parallel
    let data = dt
        .into_inner()
        .par_iter()
        .flat_map(|&pixel| {
            let alpha = ((pixel >> 24) & 0xFF) as f32 / 255.0;
            if alpha == 0.0 {
                return [
                    0u8, 0u8, 0u8,
                    //  0u8
                ];
            }

            // Calculate RGB values based on alpha channel
            [
                (((pixel >> 16) & 0xFF) as f32 / alpha).min(255.0) as u8,
                (((pixel >> 8) & 0xFF) as f32 / alpha).min(255.0) as u8,
                ((pixel & 0xFF) as f32 / alpha).min(255.0) as u8,
                // (alpha * 255.0) as u8,
            ]
        })
        .collect::<Vec<u8>>();

    // Initialize JPEG encoder with specified quality
    let mut encoder = Encoder::new(w, quality);

    encoder.set_optimized_huffman_tables(true);

    // Encode the raw RGB data directly
    encoder.encode(&data, width as u16, height as u16, ColorType::Rgb)?;

    Ok(())
}

// fn render_chunked(w: Box<dyn Write>, dt: DrawTarget, quality: u8) -> Result<(), Error> {
//     let (width, height) = (dt.width() as u32, dt.height() as u32);
//     let px: Vec<u8> = dt
//         .into_inner()
//         .par_iter()
//         .map(|&pixel| {
//             let alpha = ((pixel >> 24) & 0xFF) as f32 / 255.0;
//             if alpha == 0.0 {
//                 return [
//                     0u8, 0u8, 0u8,
//                     //  0u8
//                 ];
//             }

//             // Calculate RGB values based on alpha channel
//             [
//                 (((pixel >> 16) & 0xFF) as f32 / alpha).min(255.0) as u8,
//                 (((pixel >> 8) & 0xFF) as f32 / alpha).min(255.0) as u8,
//                 ((pixel & 0xFF) as f32 / alpha).min(255.0) as u8,
//                 // (alpha * 255.0) as u8,
//             ]
//         })
//         .flatten()
//         .collect();

//     // Create an image buffer from the RGB pixels
//     let ib = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_raw(width, height, px)
//         .context("Failed to create image buffer")?;

//     let e = JpegEncoder::new_with_quality(w, quality);
//     ib.write_with_encoder(e)
//         .context("Failed to write image with encoder")?;
//     Ok(())
// }

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::new();
    let slug = "xin-chao";

    let blog_post = fetch_blog_post(&client, slug).await?;

    let file = File::create("thumbnail.jpg").await?.try_into_std().unwrap();
    render_thumbnail(Box::new(file), &client, &blog_post).await?;

    println!("Thumbnail generated successfully!");

    Ok(())
}
