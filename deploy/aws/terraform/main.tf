# OxiGDAL AWS Terraform Configuration
# Main infrastructure configuration
# Author: COOLJAPAN OU (Team Kitasan)

terraform {
  required_version = ">= 1.6"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }

  backend "s3" {
    bucket         = "oxigdal-terraform-state"
    key            = "oxigdal/terraform.tfstate"
    region         = "us-east-1"
    encrypt        = true
    dynamodb_table = "oxigdal-terraform-locks"
  }
}

provider "aws" {
  region = var.aws_region

  default_tags {
    tags = {
      Project     = "OxiGDAL"
      Environment = var.environment
      ManagedBy   = "Terraform"
      Owner       = "COOLJAPAN OU"
    }
  }
}

# Data sources
data "aws_availability_zones" "available" {
  state = "available"
}

data "aws_caller_identity" "current" {}

# VPC Module
module "vpc" {
  source = "./modules/vpc"

  name               = "${var.project_name}-${var.environment}"
  cidr               = var.vpc_cidr
  azs                = slice(data.aws_availability_zones.available.names, 0, 2)
  private_subnets    = var.private_subnets
  public_subnets     = var.public_subnets
  enable_nat_gateway = true
  enable_dns_hostnames = true
  enable_dns_support   = true

  tags = {
    Name = "${var.project_name}-${var.environment}-vpc"
  }
}

# Security Groups Module
module "security_groups" {
  source = "./modules/security"

  vpc_id       = module.vpc.vpc_id
  project_name = var.project_name
  environment  = var.environment
}

# RDS PostgreSQL Module
module "rds" {
  source = "./modules/rds"

  identifier              = "${var.project_name}-${var.environment}"
  engine_version          = "16.1"
  instance_class          = var.db_instance_class
  allocated_storage       = var.db_allocated_storage
  storage_encrypted       = true
  db_name                 = "oxigdal"
  username                = "oxigdal"
  vpc_security_group_ids  = [module.security_groups.db_security_group_id]
  db_subnet_group_name    = module.vpc.database_subnet_group_name
  backup_retention_period = var.environment == "production" ? 7 : 1
  multi_az                = var.environment == "production"
  skip_final_snapshot     = var.environment != "production"

  tags = {
    Name = "${var.project_name}-${var.environment}-db"
  }
}

# ElastiCache Redis Module
module "redis" {
  source = "./modules/redis"

  cluster_id               = "${var.project_name}-${var.environment}"
  node_type                = var.redis_node_type
  num_cache_nodes          = var.redis_num_nodes
  parameter_group_name     = "default.redis7"
  port                     = 6379
  subnet_group_name        = module.vpc.elasticache_subnet_group_name
  security_group_ids       = [module.security_groups.redis_security_group_id]
  snapshot_retention_limit = var.environment == "production" ? 5 : 0

  tags = {
    Name = "${var.project_name}-${var.environment}-redis"
  }
}

# ECS Cluster Module
module "ecs" {
  source = "./modules/ecs"

  cluster_name = "${var.project_name}-${var.environment}"
  enable_container_insights = true

  tags = {
    Name = "${var.project_name}-${var.environment}-cluster"
  }
}

# ALB Module
module "alb" {
  source = "./modules/alb"

  name               = "${var.project_name}-${var.environment}"
  vpc_id             = module.vpc.vpc_id
  subnets            = module.vpc.public_subnets
  security_groups    = [module.security_groups.alb_security_group_id]

  target_group_config = {
    port                 = 8080
    protocol             = "HTTP"
    target_type          = "ip"
    deregistration_delay = 30
    health_check = {
      enabled             = true
      interval            = 30
      path                = "/health"
      timeout             = 5
      healthy_threshold   = 2
      unhealthy_threshold = 3
      matcher             = "200"
    }
  }

  tags = {
    Name = "${var.project_name}-${var.environment}-alb"
  }
}

# ECS Service Module
module "ecs_service" {
  source = "./modules/ecs-service"

  name            = "${var.project_name}-${var.environment}"
  cluster_id      = module.ecs.cluster_id
  desired_count   = var.ecs_desired_count
  task_cpu        = var.ecs_task_cpu
  task_memory     = var.ecs_task_memory

  container_definitions = templatefile("${path.module}/templates/container-definitions.json", {
    container_name   = "oxigdal-server"
    container_image  = var.container_image
    container_port   = 8080
    log_group        = module.cloudwatch.log_group_name
    aws_region       = var.aws_region
    postgres_host    = module.rds.endpoint
    postgres_port    = 5432
    postgres_db      = "oxigdal"
    postgres_user    = "oxigdal"
    postgres_password_arn = module.rds.password_secret_arn
    redis_host       = module.redis.endpoint
    redis_port       = 6379
  })

  vpc_id             = module.vpc.vpc_id
  subnets            = module.vpc.private_subnets
  security_groups    = [module.security_groups.ecs_security_group_id]
  target_group_arn   = module.alb.target_group_arn

  enable_autoscaling = true
  min_capacity       = var.ecs_min_capacity
  max_capacity       = var.ecs_max_capacity

  cpu_target_value    = 70
  memory_target_value = 80

  tags = {
    Name = "${var.project_name}-${var.environment}-service"
  }
}

# S3 Bucket Module
module "s3" {
  source = "./modules/s3"

  bucket_name = "${var.project_name}-${var.environment}-data-${data.aws_caller_identity.current.account_id}"

  versioning_enabled = var.environment == "production"

  lifecycle_rules = [
    {
      id      = "archive-old-data"
      enabled = true
      transitions = [
        {
          days          = 90
          storage_class = "STANDARD_IA"
        },
        {
          days          = 180
          storage_class = "GLACIER"
        }
      ]
    }
  ]

  tags = {
    Name = "${var.project_name}-${var.environment}-data"
  }
}

# CloudWatch Module
module "cloudwatch" {
  source = "./modules/cloudwatch"

  log_group_name        = "/ecs/${var.project_name}-${var.environment}"
  retention_in_days     = var.environment == "production" ? 30 : 7

  alarm_actions = var.sns_alarm_topic_arn != "" ? [var.sns_alarm_topic_arn] : []

  tags = {
    Name = "${var.project_name}-${var.environment}-logs"
  }
}

# IAM Roles Module
module "iam" {
  source = "./modules/iam"

  project_name     = var.project_name
  environment      = var.environment
  s3_bucket_arn    = module.s3.bucket_arn
  secrets_arns     = [module.rds.password_secret_arn]
}
