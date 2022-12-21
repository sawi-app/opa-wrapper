use std::{collections::HashSet, path::Path, sync::Arc};

use anyhow::Result;
use tokio::io::AsyncBufRead;
use wasmtime::{Config, Engine, Module, Store};

use opa_wasm::{AbiVersion, DefaultContext, Policy, Runtime};

#[derive(Clone)]
pub struct OPAModule {
    engine: Arc<Engine>,
    module: Arc<Module>,
}

pub struct OPAPolicy {
    policy: Policy<DefaultContext>,
    store: Store<()>,
}

impl OPAPolicy {
    pub async fn eval<V, R>(&mut self, entrypoint: &str, input: &V) -> Result<R>
    where
        V: serde::Serialize,
        R: for<'de> serde::Deserialize<'de>,
    {
        self.policy
            .evaluate(&mut self.store, entrypoint, &input)
            .await
    }

    pub fn default_entrypoint(&self) -> Option<&str> {
        self.policy.default_entrypoint()
    }

    pub fn entrypoints(&self) -> HashSet<&str> {
        self.policy.entrypoints()
    }

    pub fn abi_version(&self) -> AbiVersion {
        self.policy.abi_version()
    }
}

impl OPAModule {
    pub fn from_wasm(module: impl AsRef<[u8]>) -> Result<Self> {
        // Configure the WASM runtime
        let mut config = Config::new();
        config.async_support(true);

        let engine = Engine::new(&config)?;

        // Load the policy WASM module
        let module = Module::new(&engine, module)?;

        Ok(OPAModule {
            engine: Arc::new(engine),
            module: Arc::new(module),
        })
    }

    pub async fn from_wasm_file(path: impl AsRef<Path> + std::fmt::Debug) -> Result<Self> {
        OPAModule::from_wasm(tokio::fs::read(path).await?)
    }

    pub async fn from_bundle(reader: impl AsyncBufRead + Unpin + Send + Sync) -> Result<Self> {
        opa_wasm::load_bundle(reader)
            .await
            .and_then(|module| OPAModule::from_wasm(module))
    }

    pub async fn from_bundle_file(path: impl AsRef<Path> + std::fmt::Debug) -> Result<Self> {
        opa_wasm::read_bundle(path)
            .await
            .and_then(|module| OPAModule::from_wasm(module))
    }

    pub async fn get_policy_wasm_from_bundle(
        reader: impl AsyncBufRead + Unpin + Send + Sync,
    ) -> Result<impl AsRef<[u8]>> {
        opa_wasm::load_bundle(reader).await
    }

    pub async fn get_policy_wasm_from_bundle_file(
        path: impl AsRef<Path> + std::fmt::Debug,
    ) -> Result<impl AsRef<[u8]>> {
        opa_wasm::read_bundle(path).await
    }

    pub async fn build_policy<D>(&self, data: Option<&D>) -> Result<OPAPolicy>
    where
        D: serde::Serialize,
    {
        // Create a store which will hold the module instance
        let mut store = Store::new(&self.engine, ());

        // Instantiate the module
        let runtime = Runtime::new(&mut store, &self.module).await?;

        Ok(OPAPolicy {
            policy: match data {
                Some(data) => runtime.with_data(&mut store, data).await,
                None => runtime.without_data(&mut store).await,
            }?,
            store,
        })
    }

    pub async fn eval<D, V, R>(&self, data: Option<&D>, entrypoint: &str, input: &V) -> Result<R>
    where
        D: serde::Serialize,
        V: serde::Serialize,
        R: for<'de> serde::Deserialize<'de>,
    {
        self.build_policy::<D>(data).await?
            .eval(entrypoint, input)
            .await
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    #[tokio::test]
    async fn wasm_file() {
        let opa = OPAModule::from_wasm_file("./test_policy/policy.wasm")
            .await
            .unwrap();

        assert_eq!(
            opa.build_policy::<Value>(Some(&json!({"world": "world"})),)
                .await
                .unwrap()
                .eval::<_, Value>("example/hello", &json!({"message": "world"}))
                .await
                .unwrap(),
            json!([{"result": true}])
        );

        assert_eq!(
            opa.eval::<_, _, Value>(
                Some(&json!({"world": "world"})),
                "example/hello",
                &json!({"message": "worlds"}),
            )
            .await
            .unwrap(),
            json!([{"result": false}])
        );
    }

    #[tokio::test]
    async fn bundle_file() {
        let opa = OPAModule::from_bundle_file("./test_policy/bundle.tar.gz")
            .await
            .unwrap();

        assert_eq!(
            opa.build_policy::<Value>(Some(&json!({"world": "world"})),)
                .await
                .unwrap()
                .eval::<_, Value>("example/hello", &json!({"message": "world"}))
                .await
                .unwrap(),
            json!([{"result": true}])
        );

        assert_eq!(
            opa.eval::<_, _, Value>(
                Some(&json!({"world": "world"})),
                "example/hello",
                &json!({"message": "worlds"}),
            )
            .await
            .unwrap(),
            json!([{"result": false}])
        );
    }
}
