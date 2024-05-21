# spotify-dl-lib
A rust library which allows you to download spotify songs if you have a __premium account__

## How to use this library

### how to add the library to your cargo

(expecting your directory tree looks like this or similar)
```
├───libtester
│   └───src
├───spotify-dl-lib
    ├───src
        └───encoder
```
add this to your Cargo.toml 
```toml
[dependencies]
spotify_dl_lib = { path = "../spotify-dl", features = ["mp3"] }
```
(add the mp3 feature if you need it, it wont make the file size smaller ( for now ;) )

and yea the obvious, please do not forget to actually clone or download the repo :D

### prerequisites 
- [rust language](https://www.rust-lang.org/tools/install)

get your spotify username (**your username is not your display name**) and password ready

in case you do not know how to get your spotify username here is a quick tutorial

1. ![spotify_screenshot_1](https://github.com/mari-rs/spotify-dl-lib/assets/98649425/97bceea6-fa1d-49b0-abde-633c6f0b2e11)
2. ![spotify_Screenshot_2](https://github.com/mari-rs/spotify-dl-lib/assets/98649425/f60a48eb-a612-498b-b688-6ea95f2eac44)
3. and now you should have your spotify profile url in your clipboard, in my case, this is my profile url: https://open.spotify.com/user/xfvf8ol1ezj9bv5la7ty0vzut?si=8a8b7c4b7c5746df
4. this is the important part we need from the url: **xfvf8ol1ezj9bv5la7ty0vzut**, save this for later

### example 
```rs
use spotify_dl_lib::SpotifyDownloader;

#[tokio::main]
async fn main() {
    let username = "your username here";
    let password = "your spotify password here";

    //first argument is the name of the folder, where your mp3 files will be dropped (folder will be created in your home dir)
    let spotify_dl = SpotifyDownloader::new(&"spotify-dl-data", &username, &password).await.unwrap();

    //download a playlist, album or track!
    let tracks_to_dl = vec![
       "https://open.spotify.com/playlist/7lzJ7tSe2N6Cvbsjto4lrq?si=09e7c2f655a840c5".to_string()
    ];


    /*
    download the tracks
    (2nd argument = parallelism of how many files can be downloaded concurrently (default value is 5 if None))
    (3rd argument = compression rate (only for flac format), lower = faster! higher = takes longer due of more processing (default value is 4 if None)
    (4th argument = file format, yea the output of your file bruh (only flac and mp3 are supported at the moment)
    */
    spotify_dl.download_tracks(tracks_to_dl, None, None, "mp3").await.unwrap();

    println!("download finished!");

}
```

## Credits
This project was only possible due of the existence of [this amazing project](https://github.com/GuillemCastro/spotify-dl) <3 much love to all of you. If you are looking for a CLI solution for just downloading spotify tracks, stick to the mentioned project instead. 
