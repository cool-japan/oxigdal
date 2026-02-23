# OxiGDAL AWS Terraform Variables
# Author: COOLJAPAN OU (Team Kitasan)

variable "aws_region" {
  description = "AWS region for deployment"
  type        = string
  default     = "us-east-1"
}

variable "project_name" {
  description = "Project name"
  type        = string
  default     = "oxigdal"
}

variable "environment" {
  description = "Environment (development, staging, production)"
  type        = string
  default     = "production"

  validation {
    condition     = contains(["development", "staging", "production"], var.environment)
    error_message = "Environment must be development, staging, or production."
  }
}

# VPC Configuration
variable "vpc_cidr" {
  description = "CIDR block for VPC"
  type        = string
  default     = "10.0.0.0/16"
}

variable "private_subnets" {
  description = "Private subnet CIDR blocks"
  type        = list(string)
  default     = ["10.0.10.0/24", "10.0.11.0/24"]
}

variable "public_subnets" {
  description = "Public subnet CIDR blocks"
  type        = list(string)
  default     = ["10.0.1.0/24", "10.0.2.0/24"]
}

# RDS Configuration
variable "db_instance_class" {
  description = "RDS instance class"
  type        = string
  default     = "db.t3.medium"
}

variable "db_allocated_storage" {
  description = "RDS allocated storage in GB"
  type        = number
  default     = 20
}

# Redis Configuration
variable "redis_node_type" {
  description = "ElastiCache node type"
  type        = string
  default     = "cache.t3.medium"
}

variable "redis_num_nodes" {
  description = "Number of cache nodes"
  type        = number
  default     = 1
}

# ECS Configuration
variable "container_image" {
  description = "Docker image for OxiGDAL server"
  type        = string
  default     = "ghcr.io/cool-japan/oxigdal-server:latest"
}

variable "ecs_desired_count" {
  description = "Desired number of ECS tasks"
  type        = number
  default     = 2
}

variable "ecs_min_capacity" {
  description = "Minimum number of ECS tasks for autoscaling"
  type        = number
  default     = 2
}

variable "ecs_max_capacity" {
  description = "Maximum number of ECS tasks for autoscaling"
  type        = number
  default     = 10
}

variable "ecs_task_cpu" {
  description = "ECS task CPU units"
  type        = number
  default     = 1024
}

variable "ecs_task_memory" {
  description = "ECS task memory in MB"
  type        = number
  default     = 2048
}

# Monitoring
variable "sns_alarm_topic_arn" {
  description = "SNS topic ARN for CloudWatch alarms"
  type        = string
  default     = ""
}

# Tags
variable "tags" {
  description = "Additional tags for all resources"
  type        = map(string)
  default     = {}
}
