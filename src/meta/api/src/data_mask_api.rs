// Copyright 2021 Datafuse Labs
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use common_meta_app::data_mask::CreateDatamaskReply;
use common_meta_app::data_mask::CreateDatamaskReq;
use common_meta_app::data_mask::DropDatamaskReply;
use common_meta_app::data_mask::DropDatamaskReq;
use common_meta_app::data_mask::GetDatamaskReply;
use common_meta_app::data_mask::GetDatamaskReq;

use crate::kv_app_error::KVAppError;

#[async_trait::async_trait]
pub trait DatamaskApi: Send + Sync {
    async fn create_data_mask(
        &self,
        req: CreateDatamaskReq,
    ) -> Result<CreateDatamaskReply, KVAppError>;

    async fn drop_data_mask(&self, req: DropDatamaskReq) -> Result<DropDatamaskReply, KVAppError>;

    async fn get_data_mask(&self, req: GetDatamaskReq) -> Result<GetDatamaskReply, KVAppError>;
}
