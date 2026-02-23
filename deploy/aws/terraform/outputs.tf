# OxiGDAL AWS Terraform Outputs
# Author: COOLJAPAN OU (Team Kitasan)

output "alb_dns_name" {
  description = "DNS name of the Application Load Balancer"
  value       = module.alb.dns_name
}

output "alb_zone_id" {
  description = "Zone ID of the Application Load Balancer"
  value       = module.alb.zone_id
}

output "ecs_cluster_name" {
  description = "Name of the ECS cluster"
  value       = module.ecs.cluster_name
}

output "ecs_cluster_id" {
  description = "ID of the ECS cluster"
  value       = module.ecs.cluster_id
}

output "ecs_service_name" {
  description = "Name of the ECS service"
  value       = module.ecs_service.service_name
}

output "rds_endpoint" {
  description = "RDS database endpoint"
  value       = module.rds.endpoint
  sensitive   = true
}

output "rds_password_secret_arn" {
  description = "ARN of the Secrets Manager secret containing the RDS password"
  value       = module.rds.password_secret_arn
  sensitive   = true
}

output "redis_endpoint" {
  description = "ElastiCache Redis endpoint"
  value       = module.redis.endpoint
}

output "s3_bucket_name" {
  description = "Name of the S3 data bucket"
  value       = module.s3.bucket_name
}

output "s3_bucket_arn" {
  description = "ARN of the S3 data bucket"
  value       = module.s3.bucket_arn
}

output "vpc_id" {
  description = "ID of the VPC"
  value       = module.vpc.vpc_id
}

output "private_subnet_ids" {
  description = "IDs of private subnets"
  value       = module.vpc.private_subnets
}

output "public_subnet_ids" {
  description = "IDs of public subnets"
  value       = module.vpc.public_subnets
}

output "log_group_name" {
  description = "Name of the CloudWatch log group"
  value       = module.cloudwatch.log_group_name
}

output "task_execution_role_arn" {
  description = "ARN of the ECS task execution role"
  value       = module.iam.task_execution_role_arn
}

output "task_role_arn" {
  description = "ARN of the ECS task role"
  value       = module.iam.task_role_arn
}
