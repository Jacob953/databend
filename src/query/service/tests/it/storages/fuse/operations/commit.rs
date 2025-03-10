//  Copyright 2021 Datafuse Labs.
//
//  Licensed under the Apache License, Version 2.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
use std::any::Any;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use common_base::base::tokio;
use common_base::base::Progress;
use common_base::base::ProgressValues;
use common_catalog::catalog::Catalog;
use common_catalog::cluster_info::Cluster;
use common_catalog::database::Database;
use common_catalog::plan::DataSourcePlan;
use common_catalog::plan::PartInfoPtr;
use common_catalog::plan::Partitions;
use common_catalog::table::Table;
use common_catalog::table_context::ProcessInfo;
use common_catalog::table_context::StageAttachment;
use common_catalog::table_context::TableContext;
use common_exception::ErrorCode;
use common_exception::Result;
use common_expression::BlockThresholds;
use common_expression::DataBlock;
use common_expression::FunctionContext;
use common_io::prelude::FormatSettings;
use common_meta_app::principal::FileFormatParams;
use common_meta_app::principal::OnErrorMode;
use common_meta_app::principal::RoleInfo;
use common_meta_app::principal::UserInfo;
use common_meta_app::schema::CountTablesReply;
use common_meta_app::schema::CountTablesReq;
use common_meta_app::schema::CreateDatabaseReply;
use common_meta_app::schema::CreateDatabaseReq;
use common_meta_app::schema::CreateIndexReply;
use common_meta_app::schema::CreateIndexReq;
use common_meta_app::schema::CreateTableLockRevReply;
use common_meta_app::schema::CreateTableReply;
use common_meta_app::schema::CreateTableReq;
use common_meta_app::schema::CreateVirtualColumnReply;
use common_meta_app::schema::CreateVirtualColumnReq;
use common_meta_app::schema::DropDatabaseReply;
use common_meta_app::schema::DropDatabaseReq;
use common_meta_app::schema::DropIndexReply;
use common_meta_app::schema::DropIndexReq;
use common_meta_app::schema::DropTableByIdReq;
use common_meta_app::schema::DropTableReply;
use common_meta_app::schema::DropVirtualColumnReply;
use common_meta_app::schema::DropVirtualColumnReq;
use common_meta_app::schema::GetTableCopiedFileReply;
use common_meta_app::schema::GetTableCopiedFileReq;
use common_meta_app::schema::IndexMeta;
use common_meta_app::schema::ListIndexesReq;
use common_meta_app::schema::ListVirtualColumnsReq;
use common_meta_app::schema::RenameDatabaseReply;
use common_meta_app::schema::RenameDatabaseReq;
use common_meta_app::schema::RenameTableReply;
use common_meta_app::schema::RenameTableReq;
use common_meta_app::schema::TableIdent;
use common_meta_app::schema::TableInfo;
use common_meta_app::schema::TableMeta;
use common_meta_app::schema::TruncateTableReply;
use common_meta_app::schema::TruncateTableReq;
use common_meta_app::schema::UndropDatabaseReply;
use common_meta_app::schema::UndropDatabaseReq;
use common_meta_app::schema::UndropTableReply;
use common_meta_app::schema::UndropTableReq;
use common_meta_app::schema::UpdateTableMetaReply;
use common_meta_app::schema::UpdateTableMetaReq;
use common_meta_app::schema::UpdateVirtualColumnReply;
use common_meta_app::schema::UpdateVirtualColumnReq;
use common_meta_app::schema::UpsertTableOptionReply;
use common_meta_app::schema::UpsertTableOptionReq;
use common_meta_app::schema::VirtualColumnMeta;
use common_meta_types::MetaId;
use common_pipeline_core::InputError;
use common_settings::ChangeValue;
use common_settings::Settings;
use common_storage::DataOperator;
use common_storage::StageFileInfo;
use common_storages_fuse::io::SegmentWriter;
use common_storages_fuse::io::TableMetaLocationGenerator;
use common_storages_fuse::operations::AppendOperationLogEntry;
use common_storages_fuse::statistics::reducers::reduce_block_metas;
use common_storages_fuse::FuseTable;
use common_storages_fuse::FUSE_TBL_SNAPSHOT_PREFIX;
use dashmap::DashMap;
use databend_query::sessions::QueryContext;
use databend_query::test_kits::block_writer::BlockWriter;
use databend_query::test_kits::table_test_fixture::execute_query;
use databend_query::test_kits::table_test_fixture::TestFixture;
use futures::TryStreamExt;
use rand::thread_rng;
use rand::Rng;
use storages_common_table_meta::meta::SegmentInfo;
use storages_common_table_meta::meta::Statistics;
use walkdir::WalkDir;

use crate::storages::fuse::operations::mutation::CompactSegmentTestFixture;

#[tokio::test(flavor = "multi_thread")]
async fn test_fuse_occ_retry() -> Result<()> {
    let fixture = TestFixture::new().await;
    let db = fixture.default_db_name();
    let tbl = fixture.default_table_name();
    let ctx = fixture.ctx();
    fixture.create_default_table().await?;

    let table = fixture.latest_default_table().await?;

    // insert one row `id = 1` into the table, without committing
    {
        let num_blocks = 1;
        let rows_per_block = 1;
        let value_start_from = 1;
        let stream =
            TestFixture::gen_sample_blocks_stream_ex(num_blocks, rows_per_block, value_start_from);

        let blocks = stream.try_collect().await?;
        fixture
            .append_commit_blocks(table.clone(), blocks, false, false)
            .await?;
    }

    // insert another row `id = 5` into the table, and do commit the insertion
    {
        let num_blocks = 1;
        let rows_per_block = 1;
        let value_start_from = 5;
        let stream =
            TestFixture::gen_sample_blocks_stream_ex(num_blocks, rows_per_block, value_start_from);

        let blocks = stream.try_collect().await?;
        fixture
            .append_commit_blocks(table.clone(), blocks, false, true)
            .await?;
    }

    // let's check it out
    let qry = format!("select * from {}.{} order by id ", db, tbl);
    let blocks = execute_query(ctx.clone(), qry.as_str())
        .await?
        .try_collect::<Vec<DataBlock>>()
        .await?;

    let expected = vec![
        "+----------+----------+",
        "| Column 0 | Column 1 |",
        "+----------+----------+",
        "| 1        | (2, 3)   |",
        "| 5        | (10, 15) |",
        "+----------+----------+",
    ];
    common_expression::block_debug::assert_blocks_sorted_eq(expected, blocks.as_slice());

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_last_snapshot_hint() -> Result<()> {
    let fixture = TestFixture::new().await;
    fixture.create_default_table().await?;

    let table = fixture.latest_default_table().await?;

    let num_blocks = 1;
    let rows_per_block = 1;
    let value_start_from = 1;
    let stream =
        TestFixture::gen_sample_blocks_stream_ex(num_blocks, rows_per_block, value_start_from);

    let blocks = stream.try_collect().await?;
    fixture
        .append_commit_blocks(table.clone(), blocks, false, true)
        .await?;

    // check last snapshot hit file
    let table = fixture.latest_default_table().await?;
    let fuse_table = FuseTable::try_from_table(table.as_ref())?;
    let last_snapshot_location = fuse_table.snapshot_loc().await?.unwrap();
    let operator = fuse_table.get_operator();
    let location = fuse_table
        .meta_location_generator()
        .gen_last_snapshot_hint_location();
    let storage_meta_data = operator.info();
    let storage_prefix = storage_meta_data.root();

    let expected = format!("{}{}", storage_prefix, last_snapshot_location);
    let content = operator.read(location.as_str()).await?;

    assert_eq!(content.as_slice(), expected.as_bytes());

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_abort_on_error() -> Result<()> {
    struct Case {
        update_meta_error: Option<ErrorCode>,
        expected_error: Option<ErrorCode>,
        expected_snapshot_left: usize,
        case_name: &'static str,
        max_retry_time: Option<Duration>,
    }

    impl Case {
        async fn run(&self) -> Result<()> {
            let fixture = TestFixture::new().await;
            fixture.create_default_table().await?;
            let ctx = fixture.ctx();
            let catalog = ctx.get_catalog("default")?;

            let table = fixture.latest_default_table().await?;
            let fuse_table = FuseTable::try_from_table(table.as_ref())?;

            let overwrite = false;
            let log = vec![AppendOperationLogEntry {
                segment_location: "do not care".to_string(),
                segment_info: Arc::new(SegmentInfo::new(vec![], Statistics::default())),
            }];

            let faked_catalog = FakedCatalog {
                cat: catalog,
                error_injection: self.update_meta_error.clone(),
            };
            let ctx = Arc::new(CtxDelegation::new(ctx, faked_catalog));
            let r = fuse_table
                .commit_with_max_retry_elapsed(ctx, log, None, self.max_retry_time, overwrite)
                .await;
            if self.update_meta_error.is_some() {
                assert_eq!(
                    r.unwrap_err().code(),
                    self.expected_error.as_ref().unwrap().code(),
                    "case name {}",
                    self.case_name
                );
            } else {
                assert!(r.is_ok(), "case name {}", self.case_name);
            }

            let operator = fuse_table.get_operator();
            let table_data_prefix = fuse_table.meta_location_generator().prefix();
            let storage_meta_data = operator.info();
            let storage_prefix = storage_meta_data.root();

            let mut ss_count = 0;
            // check snapshot dir
            for entry in WalkDir::new(format!(
                "{}/{}/{}",
                storage_prefix, table_data_prefix, FUSE_TBL_SNAPSHOT_PREFIX
            )) {
                let entry = entry.unwrap();
                if entry.file_type().is_file() {
                    ss_count += 1;
                }
            }
            assert_eq!(
                ss_count, self.expected_snapshot_left,
                "case name {}",
                self.case_name
            );
            Ok(())
        }
    }

    {
        let injected_error = None;
        // no error, expect one snapshot left there
        let expected_snapshot_left = 1;
        let case = Case {
            update_meta_error: injected_error.clone(),
            expected_error: injected_error,
            expected_snapshot_left,
            case_name: "normal, not meta store error",
            max_retry_time: None,
        };
        case.run().await?;
    }

    {
        let injected_error = Some(ErrorCode::MetaStorageError("does not matter".to_owned()));
        // error may have side effects, expect one snapshot
        // left there (snapshot not removed if committing failed)
        // in case the meta store state changed (by this operation)
        let expected_snapshot_left = 1;
        let case = Case {
            update_meta_error: injected_error.clone(),
            expected_error: injected_error,
            expected_snapshot_left,
            case_name: "meta store error which may have side effects",
            max_retry_time: None,
        };
        case.run().await?;
    }

    {
        let injected_error = Some(ErrorCode::TableVersionMismatched(
            "does not matter".to_owned(),
        ));
        // if commit failed and end up with TableVersionMismatched error,
        // an OCCRetryFailure is expected to be popped up
        let expected_error = Some(ErrorCode::OCCRetryFailure("not matter".to_owned()));
        // error may have no side effects, expect no snapshot, the operation be reverted
        let expected_snapshot_left = 0;
        let case = Case {
            update_meta_error: injected_error,
            expected_error,
            expected_snapshot_left,
            case_name: "table version mismatch err",
            // shrink the test time usage
            max_retry_time: Some(Duration::from_millis(100)),
        };
        case.run().await?;
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_merge_segments() -> common_exception::Result<()> {
    let fixture = TestFixture::new().await;
    let ctx = fixture.ctx();

    let operator = ctx.get_data_operator()?.operator();
    let data_accessor = operator.clone();
    let location_gen = TableMetaLocationGenerator::with_prefix("test/".to_owned());
    let block_writer = BlockWriter::new(&data_accessor, &location_gen);
    let segment_writer = SegmentWriter::new(&data_accessor, &location_gen);

    let mut rand = thread_rng();
    let number_of_segments: usize = rand.gen_range(1..10);
    let mut block_number_of_segments = Vec::with_capacity(number_of_segments);
    let mut rows_per_blocks = Vec::with_capacity(number_of_segments);
    for _ in 0..number_of_segments {
        block_number_of_segments.push(rand.gen_range(10..30));
        rows_per_blocks.push(rand.gen_range(1..8));
    }

    let threshold = BlockThresholds {
        max_rows_per_block: 5,
        min_rows_per_block: 4,
        max_bytes_per_block: 1024,
    };

    let (locations, block_metas, segment_infos) = CompactSegmentTestFixture::gen_segments(
        &block_writer,
        &segment_writer,
        &block_number_of_segments,
        &rows_per_blocks,
        threshold,
    )
    .await?;

    let expect = reduce_block_metas(&block_metas, threshold)?;
    let iter = locations.iter().zip(segment_infos.iter());
    let (_, results) = FuseTable::merge_segments(iter)?;
    assert_eq!(expect, results);
    Ok(())
}

struct CtxDelegation {
    ctx: Arc<dyn TableContext>,
    catalog: Arc<FakedCatalog>,
}

impl CtxDelegation {
    fn new(ctx: Arc<QueryContext>, faked_cat: FakedCatalog) -> Self {
        Self {
            ctx,
            catalog: Arc::new(faked_cat),
        }
    }
}

#[async_trait::async_trait]
impl TableContext for CtxDelegation {
    fn build_table_from_source_plan(&self, _plan: &DataSourcePlan) -> Result<Arc<dyn Table>> {
        todo!()
    }

    fn incr_total_scan_value(&self, _value: ProgressValues) {
        todo!()
    }

    fn get_total_scan_value(&self) -> ProgressValues {
        todo!()
    }

    fn get_scan_progress(&self) -> Arc<Progress> {
        todo!()
    }

    fn get_scan_progress_value(&self) -> ProgressValues {
        todo!()
    }

    fn get_write_progress(&self) -> Arc<Progress> {
        self.ctx.get_write_progress()
    }

    fn get_write_progress_value(&self) -> ProgressValues {
        todo!()
    }

    fn get_result_progress(&self) -> Arc<Progress> {
        todo!()
    }

    fn get_result_progress_value(&self) -> ProgressValues {
        todo!()
    }

    fn get_status_info(&self) -> String {
        "".to_string()
    }

    fn set_status_info(&self, _info: &str) {}

    fn get_partition(&self) -> Option<PartInfoPtr> {
        todo!()
    }

    fn get_partitions(&self, _: usize) -> Vec<PartInfoPtr> {
        todo!()
    }

    fn set_partitions(&self, _partitions: Partitions) -> Result<()> {
        todo!()
    }

    fn add_partitions_sha(&self, _sha: String) {
        todo!()
    }

    fn get_partitions_shas(&self) -> Vec<String> {
        todo!()
    }

    fn get_cacheable(&self) -> bool {
        todo!()
    }

    fn set_cacheable(&self, _: bool) {
        todo!()
    }

    fn attach_query_str(&self, _kind: String, _query: String) {
        todo!()
    }

    fn get_query_str(&self) -> String {
        todo!()
    }

    fn get_fragment_id(&self) -> usize {
        todo!()
    }

    fn get_catalog(&self, _catalog_name: &str) -> Result<Arc<dyn Catalog>> {
        Ok(self.catalog.clone())
    }

    fn get_id(&self) -> String {
        self.ctx.get_id()
    }

    fn get_current_catalog(&self) -> String {
        "default".to_owned()
    }

    fn check_aborting(&self) -> Result<()> {
        todo!()
    }

    fn get_error(&self) -> Option<ErrorCode> {
        todo!()
    }

    fn get_current_database(&self) -> String {
        self.ctx.get_current_database()
    }

    fn get_current_user(&self) -> Result<UserInfo> {
        todo!()
    }

    fn get_current_role(&self) -> Option<RoleInfo> {
        todo!()
    }

    fn get_fuse_version(&self) -> String {
        todo!()
    }

    fn get_format_settings(&self) -> Result<FormatSettings> {
        todo!()
    }

    fn get_tenant(&self) -> String {
        self.ctx.get_tenant()
    }

    fn get_query_kind(&self) -> String {
        todo!()
    }

    fn get_function_context(&self) -> Result<FunctionContext> {
        todo!()
    }

    fn get_connection_id(&self) -> String {
        todo!()
    }

    fn get_settings(&self) -> Arc<Settings> {
        Settings::create("fake_settings".to_string())
    }

    fn get_shard_settings(&self) -> Arc<Settings> {
        todo!()
    }

    fn get_cluster(&self) -> Arc<Cluster> {
        todo!()
    }

    fn get_processes_info(&self) -> Vec<ProcessInfo> {
        todo!()
    }

    fn get_stage_attachment(&self) -> Option<StageAttachment> {
        todo!()
    }

    fn get_last_query_id(&self, _index: i32) -> String {
        todo!()
    }
    fn get_query_id_history(&self) -> HashSet<String> {
        todo!()
    }
    fn get_result_cache_key(&self, _query_id: &str) -> Option<String> {
        todo!()
    }
    fn set_query_id_result_cache(&self, _query_id: String, _result_cache_key: String) {
        todo!()
    }

    fn get_on_error_map(&self) -> Option<Arc<DashMap<String, HashMap<u16, InputError>>>> {
        todo!()
    }
    fn set_on_error_map(&self, _map: Arc<DashMap<String, HashMap<u16, InputError>>>) {
        todo!()
    }
    fn get_on_error_mode(&self) -> Option<OnErrorMode> {
        todo!()
    }
    fn set_on_error_mode(&self, _mode: OnErrorMode) {
        todo!()
    }
    fn get_maximum_error_per_file(&self) -> Option<HashMap<String, ErrorCode>> {
        todo!()
    }

    fn apply_changed_settings(&self, _changes: HashMap<String, ChangeValue>) -> Result<()> {
        todo!()
    }

    fn get_changed_settings(&self) -> HashMap<String, ChangeValue> {
        todo!()
    }

    fn get_data_operator(&self) -> Result<DataOperator> {
        self.ctx.get_data_operator()
    }

    fn push_precommit_block(&self, _block: DataBlock) {
        todo!()
    }

    fn consume_precommit_blocks(&self) -> Vec<DataBlock> {
        todo!()
    }

    async fn get_file_format(&self, _name: &str) -> Result<FileFormatParams> {
        todo!()
    }

    async fn get_table(
        &self,
        _catalog: &str,
        _database: &str,
        _table: &str,
    ) -> Result<Arc<dyn Table>> {
        todo!()
    }

    async fn filter_out_copied_files(
        &self,
        _catalog_name: &str,
        _database_name: &str,
        _table_name: &str,
        _files: &[StageFileInfo],
        _max_files: Option<usize>,
    ) -> Result<Vec<StageFileInfo>> {
        todo!()
    }
}

#[derive(Clone)]
struct FakedCatalog {
    cat: Arc<dyn Catalog>,
    error_injection: Option<ErrorCode>,
}

#[async_trait::async_trait]
impl Catalog for FakedCatalog {
    async fn get_database(&self, _tenant: &str, _db_name: &str) -> Result<Arc<dyn Database>> {
        todo!()
    }

    async fn list_databases(&self, _tenant: &str) -> Result<Vec<Arc<dyn Database>>> {
        todo!()
    }

    async fn create_database(&self, _req: CreateDatabaseReq) -> Result<CreateDatabaseReply> {
        todo!()
    }

    async fn drop_database(&self, _req: DropDatabaseReq) -> Result<DropDatabaseReply> {
        todo!()
    }

    async fn undrop_database(&self, _req: UndropDatabaseReq) -> Result<UndropDatabaseReply> {
        todo!()
    }

    async fn rename_database(&self, _req: RenameDatabaseReq) -> Result<RenameDatabaseReply> {
        todo!()
    }

    fn get_table_by_info(&self, table_info: &TableInfo) -> Result<Arc<dyn Table>> {
        self.cat.get_table_by_info(table_info)
    }

    async fn get_table_meta_by_id(&self, table_id: MetaId) -> Result<(TableIdent, Arc<TableMeta>)> {
        self.cat.get_table_meta_by_id(table_id).await
    }

    async fn get_table(
        &self,
        _tenant: &str,
        _db_name: &str,
        _table_name: &str,
    ) -> Result<Arc<dyn Table>> {
        todo!()
    }

    async fn list_tables(&self, _tenant: &str, _db_name: &str) -> Result<Vec<Arc<dyn Table>>> {
        todo!()
    }

    async fn list_tables_history(
        &self,
        _tenant: &str,
        _db_name: &str,
    ) -> Result<Vec<Arc<dyn Table>>> {
        todo!()
    }

    async fn create_table(&self, _req: CreateTableReq) -> Result<CreateTableReply> {
        todo!()
    }

    async fn drop_table_by_id(&self, _req: DropTableByIdReq) -> Result<DropTableReply> {
        todo!()
    }

    async fn undrop_table(&self, _req: UndropTableReq) -> Result<UndropTableReply> {
        todo!()
    }

    async fn rename_table(&self, _req: RenameTableReq) -> Result<RenameTableReply> {
        todo!()
    }

    async fn upsert_table_option(
        &self,
        _tenant: &str,
        _db_name: &str,
        _req: UpsertTableOptionReq,
    ) -> Result<UpsertTableOptionReply> {
        todo!()
    }

    async fn update_table_meta(
        &self,
        table_info: &TableInfo,
        req: UpdateTableMetaReq,
    ) -> Result<UpdateTableMetaReply> {
        if let Some(e) = &self.error_injection {
            Err(e.clone())
        } else {
            self.cat.update_table_meta(table_info, req).await
        }
    }

    async fn count_tables(&self, _req: CountTablesReq) -> Result<CountTablesReply> {
        todo!()
    }

    async fn get_table_copied_file_info(
        &self,
        _tenant: &str,
        _db_name: &str,
        _req: GetTableCopiedFileReq,
    ) -> Result<GetTableCopiedFileReply> {
        todo!()
    }

    async fn truncate_table(
        &self,
        _table_info: &TableInfo,
        _req: TruncateTableReq,
    ) -> Result<TruncateTableReply> {
        todo!()
    }

    #[async_backtrace::framed]
    async fn create_index(&self, _req: CreateIndexReq) -> Result<CreateIndexReply> {
        unimplemented!()
    }

    #[async_backtrace::framed]
    async fn drop_index(&self, _req: DropIndexReq) -> Result<DropIndexReply> {
        unimplemented!()
    }

    #[async_backtrace::framed]
    async fn list_indexes(&self, _req: ListIndexesReq) -> Result<Vec<(u64, String, IndexMeta)>> {
        unimplemented!()
    }

    #[async_backtrace::framed]
    async fn create_virtual_column(
        &self,
        _req: CreateVirtualColumnReq,
    ) -> Result<CreateVirtualColumnReply> {
        unimplemented!()
    }

    #[async_backtrace::framed]
    async fn update_virtual_column(
        &self,
        _req: UpdateVirtualColumnReq,
    ) -> Result<UpdateVirtualColumnReply> {
        unimplemented!()
    }

    #[async_backtrace::framed]
    async fn drop_virtual_column(
        &self,
        _req: DropVirtualColumnReq,
    ) -> Result<DropVirtualColumnReply> {
        unimplemented!()
    }

    #[async_backtrace::framed]
    async fn list_virtual_columns(
        &self,
        _req: ListVirtualColumnsReq,
    ) -> Result<Vec<VirtualColumnMeta>> {
        unimplemented!()
    }

    fn as_any(&self) -> &dyn Any {
        todo!()
    }

    async fn list_table_lock_revs(&self, _table_id: u64) -> Result<Vec<u64>> {
        todo!()
    }

    async fn create_table_lock_rev(
        &self,
        _expire_sec: u64,
        _table_info: &TableInfo,
    ) -> Result<CreateTableLockRevReply> {
        todo!()
    }

    async fn extend_table_lock_rev(
        &self,
        _expire_sec: u64,
        _table_info: &TableInfo,
        _revision: u64,
    ) -> Result<()> {
        todo!()
    }

    async fn delete_table_lock_rev(&self, _table_info: &TableInfo, _revision: u64) -> Result<()> {
        todo!()
    }
}
