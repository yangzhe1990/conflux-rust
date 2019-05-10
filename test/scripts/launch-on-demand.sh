mv instances instances_old
region=us-west-2
security_group_ids=sg-0345bbb6934681ea1
subnet_id=subnet-a5cfe3dc
n=$1
image="ami-0a37ce7034088596d" 
type="m5.xlarge"
keypair=$2
role=$3
res=`aws ec2 run-instances --region $region --image-id $image --count $n --key-name $keypair --instance-type $type --security-group-ids $security_group_ids --subnet-id $subnet_id --block-device-mapping DeviceName=/dev/xvda,Ebs={VolumeSize=100} --tag-specifications "ResourceType=instance,Tags=[{Key=role,Value=$role},{Key=Name,Value=$type-$image}]"`
echo $res | jq ".Instances[].InstanceId" | tr -d '"' > instances
