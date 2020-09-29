use crate::ActorType;
use activitystreams::unparsed::UnparsedMutExt;
use activitystreams_ext::UnparsedExtension;
use actix_web::HttpRequest;
use anyhow::{anyhow, Context};
use http::{header::HeaderName, HeaderMap, HeaderValue};
use http_signature_normalization::Config;
use http_signature_normalization_actix::{digest::DigestCreate, Config as ConfigActix};
use lemmy_utils::{location_info, LemmyError};
use log::debug;
use openssl::{
  hash::MessageDigest,
  pkey::PKey,
  sign::{Signer, Verifier},
};
use reqwest::{Client, Request};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, str::FromStr};
use url::Url;

lazy_static! {
  static ref CONFIG2: ConfigActix = ConfigActix::new();
  static ref HTTP_SIG_CONFIG: Config = Config::new();
}

/// Signs request headers with the given keypair.
pub async fn sign(
  client: &Client,
  mut headers: BTreeMap<String, String>,
  url: &Url,
  activity: String,
  actor_id: &Url,
  private_key: String,
) -> Result<Request, LemmyError> {
  let signing_key_id = format!("{}#main-key", actor_id);
  let digest = format!(
    "{}={}",
    Sha256::NAME,
    Sha256::new().compute(activity.as_bytes())
  );
  headers.insert("Digest".into(), digest);

  let mut path_and_query = url.path().to_string();
  if let Some(query) = url.query() {
    path_and_query = format!("{}?{}", path_and_query, query);
  }
  let signature_header_value = HTTP_SIG_CONFIG
    .begin_sign("POST", &path_and_query, headers.clone())?
    .sign(signing_key_id, |signing_string| {
      let private_key = PKey::private_key_from_pem(private_key.as_bytes())?;
      let mut signer = Signer::new(MessageDigest::sha256(), &private_key)?;
      signer.update(signing_string.as_bytes())?;

      Ok(base64::encode(signer.sign_to_vec()?)) as Result<_, LemmyError>
    })?
    .signature_header();

  let mut header_map = HeaderMap::new();
  for h in headers {
    header_map.insert(
      HeaderName::from_str(h.0.as_str())?,
      HeaderValue::from_str(h.1.as_str())?,
    );
  }
  let signed_request = client
    .post(&url.to_string())
    .headers(header_map)
    .header("Signature", signature_header_value)
    .body(activity);

  Ok(signed_request.build()?)
}

pub fn verify(request: &HttpRequest, actor: &dyn ActorType) -> Result<(), LemmyError> {
  let public_key = actor.public_key().context(location_info!())?;
  let verified = CONFIG2
    .begin_verify(
      request.method(),
      request.uri().path_and_query(),
      request.headers().clone(),
    )?
    .verify(|signature, signing_string| -> Result<bool, LemmyError> {
      debug!(
        "Verifying with key {}, message {}",
        &public_key, &signing_string
      );
      let public_key = PKey::public_key_from_pem(public_key.as_bytes())?;
      let mut verifier = Verifier::new(MessageDigest::sha256(), &public_key)?;
      verifier.update(&signing_string.as_bytes())?;
      Ok(verifier.verify(&base64::decode(signature)?)?)
    })?;

  if verified {
    debug!("verified signature for {}", &request.uri());
    Ok(())
  } else {
    Err(anyhow!("Invalid signature on request: {}", &request.uri()).into())
  }
}

// The following is taken from here:
// https://docs.rs/activitystreams/0.5.0-alpha.17/activitystreams/ext/index.html

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKey {
  pub id: String,
  pub owner: String,
  pub public_key_pem: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKeyExtension {
  pub public_key: PublicKey,
}

impl PublicKey {
  pub fn to_ext(&self) -> PublicKeyExtension {
    PublicKeyExtension {
      public_key: self.to_owned(),
    }
  }
}

impl<U> UnparsedExtension<U> for PublicKeyExtension
where
  U: UnparsedMutExt,
{
  type Error = serde_json::Error;

  fn try_from_unparsed(unparsed_mut: &mut U) -> Result<Self, Self::Error> {
    Ok(PublicKeyExtension {
      public_key: unparsed_mut.remove("publicKey")?,
    })
  }

  fn try_into_unparsed(self, unparsed_mut: &mut U) -> Result<(), Self::Error> {
    unparsed_mut.insert("publicKey", self.public_key)?;
    Ok(())
  }
}