//! Table provisioning and lifecycle helpers (`GeoTableUtil`) using official `aws_sdk_dynamodb` builders and waiters.

use std::time::Duration;

use aws_sdk_dynamodb::client::Waiters;
use aws_sdk_dynamodb::operation::create_table::builders::CreateTableInputBuilder;
use aws_sdk_dynamodb::operation::create_table::{CreateTableInput, CreateTableOutput};
use aws_sdk_dynamodb::operation::delete_table::DeleteTableOutput;
use aws_sdk_dynamodb::types::{
    AttributeDefinition, BillingMode, KeySchemaElement, KeyType, LocalSecondaryIndex, Projection,
    ProjectionType, ProvisionedThroughput, ScalarAttributeType,
};

use crate::config::GeoDataManagerConfiguration;
use crate::error::{GeoError, Result};

/// Default timeout used when waiting for DynamoDB table creation or deletion.
const DEFAULT_TABLE_WAIT_TIMEOUT: Duration = Duration::from_secs(60);

/// Utility for building `CreateTable` requests (with the `geohash-index` LSI), creating tables, and deleting tables.
#[derive(Debug, Clone)]
pub struct GeoTableUtil {
    config: GeoDataManagerConfiguration,
}

impl GeoTableUtil {
    /// Creates a new `GeoTableUtil` bound to `config`.
    #[must_use]
    pub fn new(config: GeoDataManagerConfiguration) -> Self {
        Self { config }
    }

    /// Constructs a preconfigured `CreateTableInputBuilder` with Provisioned Throughput (`10 RCU / 10 WCU`),
    /// `(hashKey [N], rangeKey [S])` primary keys, and the `geohash-index` Local Secondary Index (`ProjectionType::All`).
    pub fn get_create_table_request(&self) -> Result<CreateTableInputBuilder> {
        let base = self.build_key_and_lsi_schema()?;
        let throughput = ProvisionedThroughput::builder()
            .read_capacity_units(10)
            .write_capacity_units(10)
            .build()?;
        Ok(base.provisioned_throughput(throughput))
    }

    /// Constructs a preconfigured `CreateTableInputBuilder` using `BillingMode::PayPerRequest` (On-Demand capacity).
    pub fn get_create_table_on_demand_request(&self) -> Result<CreateTableInputBuilder> {
        let base = self.build_key_and_lsi_schema()?;
        Ok(base.billing_mode(BillingMode::PayPerRequest))
    }

    fn build_key_and_lsi_schema(&self) -> Result<CreateTableInputBuilder> {
        let hash_attr = AttributeDefinition::builder()
            .attribute_name(&self.config.hash_key_attribute_name)
            .attribute_type(ScalarAttributeType::N)
            .build()?;
        let range_attr = AttributeDefinition::builder()
            .attribute_name(&self.config.range_key_attribute_name)
            .attribute_type(ScalarAttributeType::S)
            .build()?;
        let geohash_attr = AttributeDefinition::builder()
            .attribute_name(&self.config.geohash_attribute_name)
            .attribute_type(ScalarAttributeType::N)
            .build()?;

        let pk_schema = KeySchemaElement::builder()
            .attribute_name(&self.config.hash_key_attribute_name)
            .key_type(KeyType::Hash)
            .build()?;
        let sk_schema = KeySchemaElement::builder()
            .attribute_name(&self.config.range_key_attribute_name)
            .key_type(KeyType::Range)
            .build()?;

        let lsi_pk = KeySchemaElement::builder()
            .attribute_name(&self.config.hash_key_attribute_name)
            .key_type(KeyType::Hash)
            .build()?;
        let lsi_sk = KeySchemaElement::builder()
            .attribute_name(&self.config.geohash_attribute_name)
            .key_type(KeyType::Range)
            .build()?;

        let lsi = LocalSecondaryIndex::builder()
            .index_name(&self.config.geohash_index_name)
            .key_schema(lsi_pk)
            .key_schema(lsi_sk)
            .projection(
                Projection::builder()
                    .projection_type(ProjectionType::All)
                    .build(),
            )
            .build()?;

        Ok(CreateTableInput::builder()
            .table_name(&self.config.table_name)
            .key_schema(pk_schema)
            .key_schema(sk_schema)
            .attribute_definitions(hash_attr)
            .attribute_definitions(range_attr)
            .attribute_definitions(geohash_attr)
            .local_secondary_indexes(lsi))
    }

    /// Creates the geospatial DynamoDB table if it does not already exist, waiting until its status reaches `ACTIVE`.
    /// Returns `Ok(None)` when the table already exists.
    pub async fn create_table(
        &self,
        create_table_builder: Option<CreateTableInputBuilder>,
    ) -> Result<Option<CreateTableOutput>> {
        let builder = match create_table_builder {
            Some(b) => b,
            None => self.get_create_table_request()?,
        };
        let table_name = builder
            .get_table_name()
            .clone()
            .unwrap_or_else(|| self.config.table_name.clone());

        match self
            .config
            .client
            .describe_table()
            .table_name(&table_name)
            .send()
            .await
        {
            Ok(_) => return Ok(None),
            Err(sdk_err) => {
                let is_not_found = sdk_err
                    .as_service_error()
                    .is_some_and(|e| e.is_resource_not_found_exception());
                if !is_not_found {
                    return Err(aws_sdk_dynamodb::Error::from(sdk_err).into());
                }
            }
        }

        let output = builder
            .send_with(&self.config.client)
            .await
            .map_err(aws_sdk_dynamodb::Error::from)?;

        self.config
            .client
            .wait_until_table_exists()
            .table_name(&table_name)
            .wait(DEFAULT_TABLE_WAIT_TIMEOUT)
            .await
            .map_err(|err| GeoError::InvalidParameter {
                name: "wait_until_table_exists",
                reason: err.to_string(),
            })?;

        Ok(Some(output))
    }

    /// Deletes the geospatial DynamoDB table if it exists and waits until deletion completes.
    /// Returns `Ok(None)` when the table does not exist.
    pub async fn delete_table(&self) -> Result<Option<DeleteTableOutput>> {
        let table_name = &self.config.table_name;

        let delete_res = self
            .config
            .client
            .delete_table()
            .table_name(table_name)
            .send()
            .await;

        let output = match delete_res {
            Ok(out) => out,
            Err(sdk_err) => {
                let is_not_found = sdk_err
                    .as_service_error()
                    .is_some_and(|e| e.is_resource_not_found_exception());
                if is_not_found {
                    return Ok(None);
                }
                return Err(aws_sdk_dynamodb::Error::from(sdk_err).into());
            }
        };

        self.config
            .client
            .wait_until_table_not_exists()
            .table_name(table_name)
            .wait(DEFAULT_TABLE_WAIT_TIMEOUT)
            .await
            .map_err(|err| GeoError::InvalidParameter {
                name: "wait_until_table_not_exists",
                reason: err.to_string(),
            })?;

        Ok(Some(output))
    }
}
